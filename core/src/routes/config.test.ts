import { describe, it, expect, vi, beforeAll } from 'vitest';
import request from 'supertest';
import express from 'express';

// Mock config
vi.mock('../lib/config.js', () => ({
  config: {
    llm: {
      baseUrl: 'http://localhost:1234/v1',
      apiKey: 'test-key',
      model: 'test-model',
    },
    providers: {
      activeProvider: 'openai',
      providers: {
        openai: { baseUrl: 'https://api.openai.com/v1', apiKey: 'sk-test', model: 'gpt-4o' },
      },
    },
    saveLlmConfig: vi.fn(),
    saveProviderConfig: vi.fn(),
    setActiveProvider: vi.fn(),
    getProviderConfig: vi.fn((_id) => ({
      baseUrl: 'https://api.openai.com/v1',
      apiKey: 'sk-test',
      model: 'gpt-4o',
    })),
  },
}));

// Mock ProviderFactory
vi.mock('../lib/llm/ProviderFactory.js', () => ({
  ProviderFactory: {
    createDefault: vi.fn(() => ({
      chat: vi.fn().mockResolvedValue({ content: 'Mock response' }),
    })),
    createFromModelConfig: vi.fn(() => ({
      chat: vi.fn().mockResolvedValue({ content: 'Mock provider response' }),
    })),
  },
}));

// Mock OpenAIProvider
vi.mock('../lib/llm/OpenAIProvider.js', () => {
  let baseURL: string | undefined;
  return {
    OpenAIProvider: vi.fn().mockImplementation((override?: { baseUrl?: string }) => {
      baseURL = override?.baseUrl;
      return {
        chat: vi.fn().mockImplementation(async () => {
          if (baseURL === 'http://invalid-url') {
            throw new Error('Connection failed');
          }
          return { content: 'Mock OpenAI response' };
        }),
      };
    }),
  };
});

describe('Config Routes', () => {
  let app: express.Express;

  beforeAll(async () => {
    const { createConfigRouter } = await import('./config.js');
    app = express();
    app.use(express.json());
    app.use('/api', createConfigRouter());
  });

  it('GET /api/config/llm should return current config', async () => {
    const res = await request(app).get('/api/config/llm');
    expect(res.status).toBe(200);
    expect(res.body.model).toBe('test-model');
  });

  it('POST /api/config/llm should update config', async () => {
    const newConfig = { model: 'new-model' };
    const res = await request(app).post('/api/config/llm').send(newConfig);
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
  });

  it('GET /api/providers should return providers list', async () => {
    const res = await request(app).get('/api/providers');
    expect(res.status).toBe(200);
    expect(res.body.activeProvider).toBe('openai');
    expect(Array.isArray(res.body.providers)).toBe(true);
    const openai = res.body.providers.find((p: { id: string }) => p.id === 'openai');
    expect(openai.configured).toBe(true);
    expect(openai.isActive).toBe(true);
  });

  it('PUT /api/providers/:id should update provider config', async () => {
    const res = await request(app).put('/api/providers/openai').send({ baseUrl: 'new-url' });
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
  });

  it('POST /api/providers/active should set active provider', async () => {
    const res = await request(app).post('/api/providers/active').send({ providerId: 'lmstudio' });
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
    expect(res.body.activeProvider).toBe('lmstudio');
  });

  it('POST /api/providers/:id/test should return success/fail based on connection', async () => {
    // This will hit the catch block in the route because we are not mocking OpenAIProvider
    // but the route should return success: false instead of 500.
    const res = await request(app)
      .post('/api/providers/openai/test')
      .send({ baseUrl: 'http://invalid-url', apiKey: 'test-key', model: 'test-model' });
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(false);
    expect(res.body).toHaveProperty('error');
  });
});
