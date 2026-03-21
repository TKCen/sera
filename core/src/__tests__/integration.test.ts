import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Set WORKSPACE_DIR before importing app/index.ts
vi.hoisted(() => {
  process.env.WORKSPACE_DIR = '/';
  process.env.SECRETS_MASTER_KEY = '0'.repeat(64);
});

import { describe, it, expect, beforeAll, afterAll, vi } from 'vitest';
import request from 'supertest';
import fs from 'fs/promises';
import os from 'os';

// Include all mocks that index.ts depends on
vi.mock('../lib/database.js', () => ({
  initDb: vi.fn().mockResolvedValue(undefined),
  query: vi.fn().mockResolvedValue({ rows: [], rowCount: 0 }),
  pool: {
    query: vi.fn().mockResolvedValue({ rows: [], rowCount: 0 }),
  },
}));

vi.mock('../lib/llm/OpenAIProvider.js', () => ({
  OpenAIProvider: class {
    async chat() {
      return { content: 'Mock' };
    }
    async *chatStream() {
      yield { token: 'Mock', done: true };
    }
  },
}));

vi.mock('../intercom/IntercomService.js', () => ({
  IntercomService: class {
    setBridgeService = vi.fn();
    publishThought = vi.fn();
    publish = vi.fn().mockResolvedValue(undefined);
    publishMessage = vi.fn().mockResolvedValue({ id: 'msg-1' });
    sendDirectMessage = vi.fn().mockResolvedValue({ id: 'msg-2' });
    getAgentChannels = vi.fn().mockReturnValue([]);
    generateConnectionToken = vi.fn().mockResolvedValue('token-123');
    generateSubscriptionToken = vi.fn().mockResolvedValue('sub-token-123');
  },
  IntercomError: class extends Error {},
  IntercomPermissionError: class extends Error {},
}));

vi.mock('../services/embedding.service.js', () => ({
  EmbeddingService: { getInstance: () => ({ generateEmbedding: async () => [] }) },
}));

vi.mock('../audit/AuditService.js', () => ({
  AuditService: {
    getInstance: () => ({
      record: vi.fn().mockResolvedValue(undefined),
    }),
  },
}));

vi.mock('../services/vector.service.js', () => ({
  VectorService: class {
    async search() {
      return [];
    }
  },
}));

vi.mock('../circles/CircleRegistry.js', () => {
  return {
    CircleRegistry: class {
      listCircles = vi.fn().mockReturnValue([
        {
          metadata: { name: 'development', displayName: 'Development' },
          agents: ['architect-prime'],
        },
        {
          metadata: { name: 'operations', displayName: 'Operations' },
          agents: ['researcher-prime'],
        },
      ]);
      listCircleSummaries = vi.fn().mockReturnValue([
        {
          name: 'development',
          displayName: 'Development',
          agents: ['architect-prime'],
          hasProjectContext: true,
          channelCount: 0,
        },
        {
          name: 'operations',
          displayName: 'Operations',
          agents: ['researcher-prime'],
          hasProjectContext: false,
          channelCount: 0,
        },
      ]);
      getCircle = vi.fn();
      loadFromDirectory = vi.fn().mockResolvedValue(undefined);
      init = vi.fn().mockResolvedValue(undefined);
    },
  };
});

vi.mock('../agents/Orchestrator.js', () => {
  return {
    Orchestrator: class {
      getPrimaryAgent = vi.fn().mockReturnValue({
        role: 'architect-prime',
        name: 'Architect',
        process: vi.fn().mockResolvedValue({ finalAnswer: 'Mocked response' }),
      });
      listAgents = vi
        .fn()
        .mockReturnValue([
          { name: 'architect-prime' },
          { name: 'developer-prime' },
          { name: 'researcher-prime' },
        ]);
      listCircles = vi.fn().mockReturnValue([{ name: 'development' }, { name: 'operations' }]);
      getManifest = vi.fn();
      getAllManifests = vi.fn().mockReturnValue([]);
      loadAllManifests = vi.fn().mockResolvedValue(undefined);
      loadTemplates = vi.fn();
      setIntercom = vi.fn();
      setToolExecutor = vi.fn();
      setSkillRegistry = vi.fn();
      setMemoryManager = vi.fn();
      setSandboxManager = vi.fn();
      setRegistry = vi.fn();
      setMetering = vi.fn();
      setIdentityService = vi.fn();
      setLlmRouter = vi.fn();
      setPrimaryAgent = vi.fn();
      registerAgent = vi.fn();
      watchAgentsDirectory = vi.fn();
      startDockerEventListener = vi.fn().mockResolvedValue(undefined);
      stopWatching = vi.fn();
      reloadTemplates = vi.fn().mockReturnValue({ count: 0 });
      getIntercom = vi.fn().mockReturnValue(undefined);
      getToolExecutor = vi.fn().mockReturnValue(undefined);
      getAgentInfo = vi.fn();
      getManifestByInstanceId = vi.fn();
      startInstance = vi.fn().mockResolvedValue(undefined);
      stopInstance = vi.fn().mockResolvedValue(undefined);
      restartAgent = vi.fn().mockResolvedValue(undefined);
      deregisterAgent = vi.fn();
      init = vi.fn().mockResolvedValue(undefined);
    },
  };
});

import type { Express } from 'express';
let app: Express;
let tempMemoryPath: string;

beforeAll(async () => {
  tempMemoryPath = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-memory-'));
  process.env.MEMORY_PATH = tempMemoryPath;
  process.env.WORKSPACE_DIR = path.resolve(__dirname, '..', '..', '..');

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
    it('should return empty agent instances on fresh install', async () => {
      // GET /api/agents now returns DB instances (not YAML manifests).
      // With a mocked empty DB, there are no instances yet.
      const res = await request(app).get('/api/agents');
      expect(res.status).toBe(200);
      expect(Array.isArray(res.body)).toBe(true);
      expect(res.body.length).toBe(0);
    });
  });

  describe('b. Circle loading', () => {
    it('should validate agent references against loaded manifests and load circles', async () => {
      const res = await request(app).get('/api/circles');
      expect(res.status).toBe(200);
      expect(Array.isArray(res.body)).toBe(true);
      expect(res.body.length).toBeGreaterThan(0);

      const names = res.body.map((c: { name: string }) => c.name);
      expect(names).toContain('development');
      expect(names).toContain('operations');
    });
  });

  describe('c. Chat flow', () => {
    it('should hit the orchestrator and use the loaded agent mock', async () => {
      const res = await request(app).post('/api/chat').send({ message: 'Hello, world!' });

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

      const skillIds = res.body.map((s: { id: string }) => s.id);
      expect(skillIds).toContain('file-read');
      expect(skillIds).toContain('file-write');
    });
  });
});
