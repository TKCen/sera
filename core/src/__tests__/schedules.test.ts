import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createSchedulesRouter } from '../routes/schedules.js';

// Mock the database
vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

import { query } from '../lib/database.js';

describe('Schedules API', () => {
  let app: express.Express;

  beforeEach(() => {
    vi.clearAllMocks();
    app = express();
    app.use(express.json());
    app.use('/api/schedules', createSchedulesRouter());
  });

  it('GET /api/schedules should list all schedules', async () => {
    (query as any).mockResolvedValueOnce({
      rows: [{ id: '1', name: 'Task 1', cron: '* * * * *', agent_name: 'Agent 1' }],
    });

    const res = await request(app).get('/api/schedules');
    expect(res.status).toBe(200);
    expect(res.body).toHaveLength(1);
    expect(res.body[0].name).toBe('Task 1');
  });

  it('POST /api/schedules should create a new schedule', async () => {
    (query as any).mockResolvedValueOnce({ rowCount: 1 }); // INSERT
    (query as any).mockResolvedValueOnce({
      // SELECT back
      rows: [{ id: 'new-id', name: 'New Task', agent_name: 'Agent 1' }],
    });

    const payload = {
      agentId: 'agent-uuid',
      name: 'New Task',
      cron: '0 0 * * *',
      task: { action: 'test' },
    };

    const res = await request(app).post('/api/schedules').send(payload);
    expect(res.status).toBe(201);
    expect(res.body.name).toBe('New Task');
    expect(query).toHaveBeenCalledTimes(2);
  });

  it('DELETE /api/schedules/:id should delete a schedule', async () => {
    (query as any).mockResolvedValueOnce({ rowCount: 1 });

    const res = await request(app).delete('/api/schedules/123');
    expect(res.status).toBe(204);
  });
});
