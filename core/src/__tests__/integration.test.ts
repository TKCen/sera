import { describe, it, expect, beforeAll, vi } from 'vitest';

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

beforeAll(async () => {
  const appModule = await import('../index.js');
  app = appModule.app;
});

describe('Integration Test with Index Import', () => {
  it('should pass if index.js is imported', () => {
    expect(app).toBeDefined();
  });
});
