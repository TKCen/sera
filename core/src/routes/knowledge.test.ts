import { describe, it, expect, vi, beforeEach } from 'vitest';
import express from 'express';
import request from 'supertest';
import { createKnowledgeRouter } from './knowledge.js';

// Mock KnowledgeGitService
vi.mock('../memory/KnowledgeGitService.js', () => {
  const mockInstance = {
    log: vi.fn(),
    listMergeRequests: vi.fn(),
    approveMergeRequest: vi.fn(),
    resolveMergeConflict: vi.fn(),
  };
  return {
    KnowledgeGitService: {
      getInstance: () => mockInstance,
    },
  };
});

// Mock Logger
vi.mock('../lib/logger.js', () => ({
  Logger: class {
    info = vi.fn();
    error = vi.fn();
    warn = vi.fn();
    debug = vi.fn();
  },
}));

import { KnowledgeGitService } from '../memory/KnowledgeGitService.js';

function createApp(llmRouter?: unknown) {
  const app = express();
  app.use(express.json());
  app.use('/api/knowledge', createKnowledgeRouter(llmRouter as never));
  return app;
}

describe('Knowledge routes', () => {
  const gitService = KnowledgeGitService.getInstance() as unknown as {
    log: ReturnType<typeof vi.fn>;
    listMergeRequests: ReturnType<typeof vi.fn>;
    approveMergeRequest: ReturnType<typeof vi.fn>;
    resolveMergeConflict: ReturnType<typeof vi.fn>;
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('GET /api/knowledge/circles/:id/history', () => {
    it('returns log entries', async () => {
      const entries = [{ commitHash: 'abc123', authorName: 'test', timestamp: '2026-01-01' }];
      gitService.log.mockResolvedValueOnce(entries);

      const app = createApp();
      const res = await request(app).get('/api/knowledge/circles/circle-1/history');

      expect(res.status).toBe(200);
      expect(res.body).toEqual(entries);
      expect(gitService.log).toHaveBeenCalledWith('circle-1');
    });
  });

  describe('GET /api/knowledge/circles/:id/merge-requests', () => {
    it('returns merge requests', async () => {
      const requests = [{ id: 'mr-1', status: 'pending' }];
      gitService.listMergeRequests.mockResolvedValueOnce(requests);

      const app = createApp();
      const res = await request(app).get('/api/knowledge/circles/circle-1/merge-requests');

      expect(res.status).toBe(200);
      expect(res.body).toEqual(requests);
    });
  });

  describe('POST /api/knowledge/circles/:id/merge-requests/:requestId/resolve', () => {
    it('rejects invalid strategy', async () => {
      const app = createApp();
      const res = await request(app)
        .post('/api/knowledge/circles/circle-1/merge-requests/mr-1/resolve')
        .send({ strategy: 'invalid' });

      expect(res.status).toBe(400);
      expect(res.body.error).toMatch(/Invalid strategy/);
    });

    it('resolves with "ours" strategy', async () => {
      gitService.resolveMergeConflict.mockResolvedValueOnce({
        strategy: 'ours',
        filesResolved: ['doc.md'],
        commitHash: 'abc123',
      });

      const app = createApp();
      const res = await request(app)
        .post('/api/knowledge/circles/circle-1/merge-requests/mr-1/resolve')
        .send({ strategy: 'ours' });

      expect(res.status).toBe(200);
      expect(res.body.success).toBe(true);
      expect(res.body.strategy).toBe('ours');
      expect(res.body.filesResolved).toEqual(['doc.md']);
      expect(gitService.resolveMergeConflict).toHaveBeenCalledWith(
        'mr-1',
        'ours',
        'operator',
        undefined
      );
    });

    it('resolves with "theirs" strategy', async () => {
      gitService.resolveMergeConflict.mockResolvedValueOnce({
        strategy: 'theirs',
        filesResolved: ['notes.md', 'readme.md'],
        commitHash: 'def456',
      });

      const app = createApp();
      const res = await request(app)
        .post('/api/knowledge/circles/circle-1/merge-requests/mr-1/resolve')
        .send({ strategy: 'theirs' });

      expect(res.status).toBe(200);
      expect(res.body.filesResolved).toHaveLength(2);
    });

    it('returns 503 for "llm" strategy when no LLM router', async () => {
      const app = createApp(); // no llmRouter
      const res = await request(app)
        .post('/api/knowledge/circles/circle-1/merge-requests/mr-1/resolve')
        .send({ strategy: 'llm' });

      expect(res.status).toBe(503);
      expect(res.body.error).toMatch(/unavailable/);
    });

    it('resolves with "llm" strategy when LLM router provided', async () => {
      gitService.resolveMergeConflict.mockResolvedValueOnce({
        strategy: 'llm',
        filesResolved: ['knowledge.md'],
        commitHash: 'ghi789',
      });

      const mockLlmRouter = {
        getRegistry: () => ({ getDefaultModel: () => 'test-model' }),
        chatCompletion: vi.fn().mockResolvedValue({
          response: {
            choices: [{ message: { content: 'merged content' } }],
          },
        }),
      };

      const app = createApp(mockLlmRouter);
      const res = await request(app)
        .post('/api/knowledge/circles/circle-1/merge-requests/mr-1/resolve')
        .send({ strategy: 'llm' });

      expect(res.status).toBe(200);
      expect(res.body.strategy).toBe('llm');
      // The llmMergeFn should have been passed to resolveMergeConflict
      expect(gitService.resolveMergeConflict).toHaveBeenCalledWith(
        'mr-1',
        'llm',
        'operator',
        expect.any(Function)
      );
    });

    it('returns 500 on service error', async () => {
      gitService.resolveMergeConflict.mockRejectedValueOnce(
        new Error('Merge request mr-99 not found')
      );

      const app = createApp();
      const res = await request(app)
        .post('/api/knowledge/circles/circle-1/merge-requests/mr-1/resolve')
        .send({ strategy: 'ours' });

      expect(res.status).toBe(500);
      expect(res.body.error).toMatch(/not found/);
    });
  });
});
