import { describe, it, expect, beforeAll, afterAll, vi } from 'vitest';
import request from 'supertest';
import fs from 'fs/promises';
import os from 'os';
import path from 'path';

// Mock database initialization to avoid connecting to actual PostgreSQL
vi.mock('../lib/database.js', () => ({
  initDb: vi.fn().mockResolvedValue(undefined),
  query: vi.fn().mockResolvedValue({ rows: [], rowCount: 0 }),
}));

// Mock the LLM provider to avoid real API calls in /api/chat
vi.mock('../lib/llm/OpenAIProvider.js', () => {
  return {
    OpenAIProvider: class {
      async chat() {
        return {
          content: 'Mocked response',
          usage: { promptTokens: 10, completionTokens: 5, totalTokens: 15 },
        };
      }
      async *chatStream() {
        yield { token: 'Mocked response', done: false };
        yield { token: '', done: true };
      }
    }
  };
});

// Mock Qdrant/vector dependencies
vi.mock('../services/embedding.service.js', () => ({
  EmbeddingService: {
    getInstance: vi.fn().mockReturnValue({
      generateEmbedding: vi.fn().mockResolvedValue([0.1, 0.2, 0.3]),
    }),
  },
}));

vi.mock('../services/vector.service.js', () => ({
  VectorService: vi.fn().mockImplementation(() => ({
    search: vi.fn().mockResolvedValue([]),
  })),
}));

// Mock IntercomService to avoid HTTP calls to Centrifugo
vi.mock('../intercom/IntercomService.js', () => {
  class MockIntercomService {
    publish = vi.fn().mockResolvedValue(undefined);
    publishThought = vi.fn().mockResolvedValue(undefined);
    publishStreamToken = vi.fn().mockResolvedValue(undefined);
    publishMessage = vi.fn().mockResolvedValue({ id: 'mock', timestamp: new Date().toISOString() });
    sendDirectMessage = vi.fn().mockResolvedValue({ id: 'mock', timestamp: new Date().toISOString() });
    presence = vi.fn().mockResolvedValue({});
    getHistory = vi.fn().mockResolvedValue([]);
    getAgentChannels = vi.fn().mockReturnValue({
      thoughts: 'mock:thoughts', terminal: 'mock:terminal',
      publishChannels: [], subscribeChannels: [], dmPeers: [],
    });
    publishToCircleChannel = vi.fn().mockResolvedValue({ id: 'mock' });
  }
  class MockIntercomError extends Error {
    channel: string;
    constructor(msg: string, channel: string) { super(msg); this.channel = channel; }
  }
  class MockIntercomPermissionError extends MockIntercomError {
    constructor(from: string, to: string) { super(`${from} -> ${to}`, `dm:${from}:${to}`); }
  }
  return {
    IntercomService: MockIntercomService,
    IntercomError: MockIntercomError,
    IntercomPermissionError: MockIntercomPermissionError,
  };
});

// Set up a temporary directory for memory storage before importing the app
let tempMemoryPath: string;
let app: any;

beforeAll(async () => {
  tempMemoryPath = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-memory-'));
  process.env.MEMORY_PATH = tempMemoryPath;

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

      if (res.status !== 200) {
        console.error('500 ERROR CAUSE:', res.body);
      }

      expect(res.status).toBe(200);
      expect(res.body).toHaveProperty('reply');
      expect(typeof res.body.reply).toBe('string');
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
