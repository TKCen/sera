import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createOperatorRequestsRouter } from './operator-requests.js';

vi.mock('../auth/authMiddleware.js', () => ({
  requireRole: vi.fn(() => (req: any, res: any, next: any) => next()),
}));

vi.mock('../middleware/rateLimit.js', () => ({
  rateLimit: vi.fn((req: any, res: any, next: any) => next()),
}));

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
  },
}));

import { pool } from '../lib/database.js';

describe('Operator Requests Routes', () => {
  let app: express.Express;

  beforeEach(() => {
    app = express();
    app.use(express.json());
    app.use('/api/operator-requests', createOperatorRequestsRouter());
  });

  it('GET /api/operator-requests/pending/count returns count', async () => {
    vi.mocked(pool.query).mockResolvedValue({ rows: [{ count: 5 }] } as any);
    const res = await request(app).get('/api/operator-requests/pending/count');
    expect(res.status).toBe(200);
    expect(res.body.count).toBe(5);
  });

  it('GET /api/operator-requests returns list', async () => {
    vi.mocked(pool.query).mockResolvedValue({ rows: [] } as any);
    const res = await request(app).get('/api/operator-requests');
    expect(res.status).toBe(200);
    expect(res.body).toEqual([]);
  });
});
