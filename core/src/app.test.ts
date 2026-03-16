import { describe, it, expect } from 'vitest';
import request from 'supertest';
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
