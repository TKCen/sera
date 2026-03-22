import { describe, it, expect, vi, afterAll } from 'vitest';
import request from 'supertest';

vi.mock('./services/vector.service.js', () => ({
  VectorService: class {
    async search() {
      return [];
    }
    async upsertPoints() {}
    async deletePoints() {}
  },
}));
vi.hoisted(() => {
  process.env.SECRETS_MASTER_KEY = '0'.repeat(64);
});

import { app } from './index.js';

describe('SERA Core API', () => {
  // Allow pending async operations (circle loading, etc.) to settle
  // before vitest tears down the worker, preventing EnvironmentTeardownError.
  afterAll(async () => {
    await new Promise((resolve) => setTimeout(resolve, 100));
  });
  it('GET /api/health should return 200 and status ok', async () => {
    const response = await request(app).get('/api/health');

    expect(response.status).toBe(200);
    expect(response.body).toHaveProperty('status', 'ok');
    expect(response.body).toHaveProperty('service', 'sera-core');
    expect(response.body).toHaveProperty('timestamp');
  });

  it('GET /non-existent should return 404', async () => {
    const response = await request(app).get('/non-existent');
    expect(response.status).toBe(404);
  });
});
