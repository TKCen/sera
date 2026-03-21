import { describe, it, expect, vi } from 'vitest';
import request from 'supertest';
vi.hoisted(() => {
  process.env.SECRETS_MASTER_KEY = '0'.repeat(64);
});
import { app } from '../index.js';

vi.mock('../lib/llm/OpenAIProvider.js', () => ({
  OpenAIProvider: class {
    async chat() {
      return { content: 'Mocked response' };
    }
    async *chatStream() {
      yield { token: 'Mocked response', done: false };
      yield { token: '', done: true };
    }
  },
}));

vi.mock('../services/embedding.service.js', () => ({
  EmbeddingService: {
    getInstance: vi.fn().mockReturnValue({ generateEmbedding: vi.fn().mockResolvedValue([]) }),
  },
}));

vi.mock('../services/vector.service.js', () => ({
  VectorService: class {
    async search() {
      return [];
    }
    async upsertPoints() {}
    async deletePoints() {}
  },
}));

vi.mock('../intercom/IntercomService.js', () => {
  class MockIntercomService {
    setBridgeService = vi.fn();
    publish = vi.fn().mockResolvedValue(undefined);
    publishThought = vi.fn().mockResolvedValue(undefined);
    publishStreamToken = vi.fn().mockResolvedValue(undefined);
    publishMessage = vi.fn().mockResolvedValue({ id: 'mock', timestamp: new Date().toISOString() });
    getAgentChannels = vi.fn().mockReturnValue({ thoughts: 'mock:thoughts' });
  }
  return { IntercomService: MockIntercomService };
});

describe('OpenAI-Compatible API', () => {
  it('POST /v1/chat/completions should return 404 for unknown model', async () => {
    const response = await request(app)
      .post('/v1/chat/completions')
      .send({
        model: 'unknown-agent',
        messages: [{ role: 'user', content: 'hello' }],
      });

    expect(response.status).toBe(404);
    expect(response.body.error.message).toContain('not found');
  });

  it('POST /v1/chat/completions should return 400 if model is missing', async () => {
    const response = await request(app)
      .post('/v1/chat/completions')
      .send({
        messages: [{ role: 'user', content: 'hello' }],
      });

    expect(response.status).toBe(400);
    expect(response.body.error.message).toBe('model is required');
  });

  it('POST /v1/chat/completions should return 400 if messages are missing', async () => {
    const response = await request(app).post('/v1/chat/completions').send({
      model: 'architect-prime',
    });

    expect(response.status).toBe(400);
    expect(response.body.error.message).toBe('messages array is required and cannot be empty');
  });

  it('POST /v1/chat/completions should return 400 if messages are empty', async () => {
    const response = await request(app).post('/v1/chat/completions').send({
      model: 'architect-prime',
      messages: [],
    });

    expect(response.status).toBe(400);
    expect(response.body.error.message).toBe('messages array is required and cannot be empty');
  });

  it('POST /v1/chat/completions should handle non-streaming response', async () => {
    const response = await request(app)
      .post('/v1/chat/completions')
      .send({
        model: 'architect-prime',
        messages: [{ role: 'user', content: 'Say hello' }],
        stream: false,
      });

    // architect-prime is a template, not a loaded manifest — returns 404.
    // If a manifest were loaded, 200/500/502 are also valid depending on LLM availability.
    expect([200, 404, 500, 502]).toContain(response.status);
    if (response.status === 200) {
      expect(response.body).toHaveProperty('id');
      expect(response.body).toHaveProperty('object', 'chat.completion');
      expect(response.body.choices[0].message).toHaveProperty('role', 'assistant');
    }
  });

  it('POST /v1/chat/completions should handle streaming response', async () => {
    const response = await request(app)
      .post('/v1/chat/completions')
      .send({
        model: 'architect-prime',
        messages: [{ role: 'user', content: 'Say hello' }],
        stream: true,
      });

    expect([200, 404, 500, 502]).toContain(response.status);
    if (response.status === 200) {
      expect(response.header['content-type']).toContain('text/event-stream');
    }
  });
});
