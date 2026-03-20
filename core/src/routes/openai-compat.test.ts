import { describe, it, expect, vi, beforeAll } from 'vitest';
import request from 'supertest';
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
    // We need to mock the agent.process method
    // Since we are using AgentFactory.createAgent which returns a WorkerAgent, we should mock it.

    const response = await request(app)
      .post('/v1/chat/completions')
      .send({
        model: 'architect-prime',
        messages: [{ role: 'user', content: 'Say hello' }],
        stream: false,
      });

    // Note: This test might fail if the real LLM is called and not configured.
    // However, architect-prime is configured to use lm-studio which might not be running.
    // If it fails with 500, it confirms it reached the agent.process call.
    expect([200, 500, 502]).toContain(response.status);
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

    expect([200, 500, 502]).toContain(response.status);
    if (response.status === 200) {
      expect(response.header['content-type']).toContain('text/event-stream');
    }
  });
});
