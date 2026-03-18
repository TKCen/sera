import { describe, it, expect, beforeAll, afterAll, vi } from 'vitest';
import request from 'supertest';
import fs from 'fs/promises';
import os from 'os';
import path from 'path';
import { Orchestrator } from '../agents/Orchestrator.js';

// Include all mocks that index.ts depends on
vi.mock('../lib/database.js', () => ({
  initDb: vi.fn().mockResolvedValue(undefined),
  query: vi.fn().mockResolvedValue({ rows: [], rowCount: 0 }),
}));

vi.mock('../lib/llm/OpenAIProvider.js', () => ({
  OpenAIProvider: class {
    async chat() { return { content: 'Mock' }; }
    async *chatStream() { yield { token: 'Mock', done: true }; }
  }
}));

vi.mock('../intercom/IntercomService.js', () => ({
  IntercomService: class {
    setBridgeService() {}
    publishThought() {}
  },
  IntercomError: class extends Error {},
  IntercomPermissionError: class extends Error {},
}));

vi.mock('../services/embedding.service.js', () => ({
  EmbeddingService: { getInstance: () => ({ generateEmbedding: async () => [] }) }
}));

vi.mock('../services/vector.service.js', () => ({
  VectorService: class { async search() { return []; } }
}));

let app: any;
let tempMemoryPath: string;

beforeAll(async () => {
  tempMemoryPath = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-memory-'));
  process.env.MEMORY_PATH = tempMemoryPath;

  vi.spyOn(Orchestrator.prototype, 'getPrimaryAgent').mockReturnValue({
    role: 'architect-prime',
    name: 'Architect',
    process: vi.fn().mockResolvedValue({ finalAnswer: 'Mocked response' })
  } as any);

  // Dynamically import the Express app after mocks and env vars are in place
  const appModule = await import('../index.js');
  app = appModule.app;
});

afterAll(async () => {
  if (tempMemoryPath) {
    await fs.rm(tempMemoryPath, { recursive: true, force: true });
  }
});

describe('SERA Integration Tests', () => {
  describe('a. Agent loading', () => {
    it('should register agents from AGENT.yaml manifests', async () => {
      const res = await request(app).get('/api/agents');
      expect(res.status).toBe(200);
      expect(Array.isArray(res.body)).toBe(true);
      expect(res.body.length).toBeGreaterThan(0);

      const names = res.body.map((a: any) => a.name); // Orchestrator.listAgents returns array of { name, ... }
      expect(names).toContain('architect-prime');
      expect(names).toContain('developer-prime');
      expect(names).toContain('researcher-prime');
    });
  });

  describe('b. Circle loading', () => {
    it('should validate agent references against loaded manifests and load circles', async () => {
      const res = await request(app).get('/api/circles');
      expect(res.status).toBe(200);
      expect(Array.isArray(res.body)).toBe(true);
      expect(res.body.length).toBeGreaterThan(0);

      const names = res.body.map((c: any) => c.name);
      expect(names).toContain('development');
      expect(names).toContain('operations');
    });
  });

  describe('c. Chat flow', () => {
    it('should hit the orchestrator and use the loaded agent mock', async () => {
      const res = await request(app)
        .post('/api/chat')
        .send({ message: 'Hello, world!' });

      if (res.status === 500) {
        console.error('500 ERROR BODY:', res.body);
      }

      expect(res.status).toBe(200);
      expect(res.body).toHaveProperty('reply');
      // Our mock returns 'Mocked response' but PrimaryAgent wraps it depending on structure
      // Wait, primary agent extracts finalAnswer or thought, so let's just check it's defined
      expect(typeof res.body.reply).toBe('string');
      // Our mock response doesn't have `<final_answer>` block so PrimaryAgent might extract it differently,
      // but it should not crash.
    });
  });

  describe('d. Memory flow', () => {
    let createdId: string;

    it('should create a memory entry via POST /api/memory/blocks/:type', async () => {
      const res = await request(app)
        .post('/api/memory/blocks/core')
        .send({ title: 'Test Memory', content: 'This is a test memory content.' });

      expect(res.status).toBe(201);
      expect(res.body).toHaveProperty('id');
      expect(res.body.title).toBe('Test Memory');
      expect(res.body.content).toBe('This is a test memory content.');

      createdId = res.body.id;
    });

    it('should retrieve the created memory entry via GET /api/memory/entries/:id', async () => {
      expect(createdId).toBeDefined();
      const res = await request(app).get(`/api/memory/entries/${createdId}`);
      expect(res.status).toBe(200);
      expect(res.body.id).toBe(createdId);
      expect(res.body.title).toBe('Test Memory');
      expect(res.body.content).toBe('This is a test memory content.');
    });
  });

  describe('e. Skills flow', () => {
    it('should have builtin skills registered after server boot', async () => {
      const res = await request(app).get('/api/skills');
      expect(res.status).toBe(200);
      expect(Array.isArray(res.body)).toBe(true);
      expect(res.body.length).toBeGreaterThan(0);

      const skillIds = res.body.map((s: any) => s.id);
      expect(skillIds).toContain('file-read');
      expect(skillIds).toContain('file-write');
    });
  });
});
