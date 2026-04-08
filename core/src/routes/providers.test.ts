import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createProvidersRouter } from './providers.js';

vi.mock('../auth/authMiddleware.js', () => ({
  requireRole: vi.fn(() => (req: any, res: any, next: any) => next()),
}));

describe('Providers Routes', () => {
  let app: express.Express;
  let llmRouterMock: any;
  let circuitBreakerServiceMock: any;
  let dynamicProviderManagerMock: any;

  beforeEach(() => {
    llmRouterMock = {
      listModels: vi.fn().mockResolvedValue([]),
      addModel: vi.fn().mockResolvedValue({}),
      deleteModel: vi.fn().mockResolvedValue(undefined),
      testModel: vi.fn().mockResolvedValue({ ok: true, latencyMs: 100 }),
      getRegistry: vi.fn().mockReturnValue({
        list: vi.fn().mockReturnValue([]),
        getDefaultModel: vi.fn().mockReturnValue('test-model'),
        setDefaultModel: vi.fn(),
      }),
    };
    circuitBreakerServiceMock = {
      getProviderState: vi.fn(),
    };
    dynamicProviderManagerMock = {
      listProviders: vi.fn().mockReturnValue([]),
      getStatuses: vi.fn().mockReturnValue({}),
      addProvider: vi.fn().mockResolvedValue({}),
      removeProvider: vi.fn().mockResolvedValue(undefined),
      testConnection: vi.fn().mockResolvedValue({ success: true }),
    };

    app = express();
    app.use(express.json());
    app.use(
      '/api/providers',
      createProvidersRouter(llmRouterMock, circuitBreakerServiceMock, dynamicProviderManagerMock)
    );
  });

  it('GET /api/providers returns list', async () => {
    const res = await request(app).get('/api/providers');
    expect(res.status).toBe(200);
  });

  it('PUT /api/providers/default-model updates default model', async () => {
    const res = await request(app)
      .put('/api/providers/default-model')
      .send({ modelName: 'new-model' });
    expect(res.status).toBe(200);
    expect(llmRouterMock.getRegistry().setDefaultModel).toHaveBeenCalledWith('new-model');
  });

  it('PUT /api/providers/default-model returns 400 for invalid body', async () => {
    const res = await request(app).put('/api/providers/default-model').send({});
    expect(res.status).toBe(400);
  });
});
