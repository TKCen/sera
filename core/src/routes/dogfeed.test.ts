import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock the dogfeed modules before importing the router
vi.mock('../dogfeed/loop.js', () => {
  return {
    DogfeedLoop: class {
      getStatus = vi.fn().mockReturnValue({ phase: 'idle' });
      getLastResult = vi.fn().mockReturnValue(undefined);
      runCycle = vi.fn().mockResolvedValue({
        success: true,
        task: { priority: 1, category: 'lint', description: 'test task', status: 'done' },
        agent: 'pi-agent',
        branch: 'dogfeed/1-test',
        ciPassed: true,
        merged: true,
        durationMs: 5000,
        estimatedTokens: 0,
        filesChanged: 1,
        linesAdded: 2,
        linesRemoved: 1,
      });
    },
  };
});

vi.mock('../dogfeed/analyzer.js', () => {
  return {
    DogfeedAnalyzer: class {
      scanTaskFile = vi.fn().mockReturnValue([
        { priority: 0, category: 'lint', description: 'Fix lint', status: 'ready' },
        { priority: 1, category: 'test', description: 'Add test', status: 'ready' },
      ]);
      pickNext = vi.fn().mockReturnValue({
        priority: 0,
        category: 'lint',
        description: 'Fix lint',
        status: 'ready',
      });
    },
  };
});

vi.mock('../dogfeed/constants.js', async () => {
  const actual =
    await vi.importActual<typeof import('../dogfeed/constants.js')>('../dogfeed/constants.js');
  return {
    ...actual,
    createDefaultConfig: vi.fn((overrides) => ({
      repoRoot: '/tmp/test',
      taskFile: '/tmp/test/docs/DOGFEED-TASKS.md',
      phaseLog: '/tmp/test/docs/DOGFEED-PHASE0-LOG.md',
      agentTimeoutMs: 1800000,
      piAgentModel: 'qwen/qwen3.5-35b-a3b',
      piAgentProvider: 'lmstudio',
      pushToRemote: false,
      autoMerge: false,
      gitUserName: 'test',
      gitUserEmail: 'test@test.com',
      ...overrides,
    })),
  };
});

import express from 'express';
import request from 'supertest';
import { createDogfeedRouter } from './dogfeed.js';

describe('dogfeed routes', () => {
  let app: express.Express;

  beforeEach(() => {
    app = express();
    app.use(express.json());
    app.use('/api/dogfeed', createDogfeedRouter());
  });

  describe('GET /api/dogfeed/status', () => {
    it('returns idle status', async () => {
      const res = await request(app).get('/api/dogfeed/status');
      expect(res.status).toBe(200);
      expect(res.body.running).toBe(false);
      expect(res.body.status.phase).toBe('idle');
    });
  });

  describe('GET /api/dogfeed/tasks', () => {
    it('returns task list', async () => {
      const res = await request(app).get('/api/dogfeed/tasks');
      expect(res.status).toBe(200);
      expect(res.body.total).toBe(2);
      expect(res.body.ready).toBe(2);
    });
  });

  describe('GET /api/dogfeed/next', () => {
    it('returns the next task', async () => {
      const res = await request(app).get('/api/dogfeed/next');
      expect(res.status).toBe(200);
      expect(res.body.available).toBe(true);
      expect(res.body.task.category).toBe('lint');
    });
  });

  describe('POST /api/dogfeed/run', () => {
    it('triggers a cycle and returns result', async () => {
      const res = await request(app).post('/api/dogfeed/run');
      expect(res.status).toBe(200);
      expect(res.body.success).toBe(true);
      expect(res.body.agent).toBe('pi-agent');
    });
  });
});
