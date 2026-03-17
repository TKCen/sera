import { describe, it, expect, vi } from 'vitest';
import request from 'supertest';

vi.mock('./services/vector.service.js', () => ({
  VectorService: class {
    async search() { return []; }
    async upsertPoints() {}
    async deletePoints() {}
  }
}));

import { app } from './index.js';

describe('SERA Core API', () => {
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
