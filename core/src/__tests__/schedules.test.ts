import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createSchedulesRouter } from '../routes/schedules.js';
import { pool } from '../lib/database.js';
import { ScheduleService, type Schedule } from '../services/ScheduleService.js';
import type { QueryResult } from 'pg';

// Mock the database
vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn<any>().mockResolvedValue({ rows: [] }),
  },
  query: vi.fn<any>().mockResolvedValue({ rows: [] }),
}));

// Mock ScheduleService
vi.mock('../services/ScheduleService.js', () => ({
  ScheduleService: {
    getInstance: vi.fn().mockReturnValue({
      createSchedule: vi.fn(),
      deleteSchedule: vi.fn(),
    }),
  },
}));

describe('Schedules API', () => {
  let app: express.Express;
  let mockScheduleService: ReturnType<typeof ScheduleService.getInstance>;

  beforeEach(() => {
    vi.clearAllMocks();
    mockScheduleService = ScheduleService.getInstance() as ReturnType<
      typeof ScheduleService.getInstance
    >;
    app = express();
    app.use(express.json());
    app.use('/api/schedules', createSchedulesRouter());
  });

  it('GET /api/schedules should list all schedules', async () => {
    (pool.query as any).mockResolvedValueOnce({
      rows: [{ id: '1', name: 'Task 1', cron: '* * * * *', agent_name: 'Agent 1' }],
    } as unknown as QueryResult<any>);

    const res = await request(app).get('/api/schedules');
    expect(res.status).toBe(200);
    expect(res.body).toHaveLength(1);
    expect(res.body[0].name).toBe('Task 1');
  });

  it('POST /api/schedules should create a new schedule', async () => {
    const payload = {
      agentId: 'agent-uuid',
      name: 'New Task',
      cron: '0 0 * * *',
      task: { action: 'test' },
    };

    vi.mocked(mockScheduleService.createSchedule).mockResolvedValueOnce({
      id: 'new-id',
      name: 'New Task',
      agent_name: 'Agent 1',
    } as unknown as Schedule);

    const res = await request(app).post('/api/schedules').send(payload);
    expect(res.status).toBe(201);
    expect(res.body.name).toBe('New Task');
    expect(mockScheduleService.createSchedule).toHaveBeenCalledWith(payload);
  });

  it('DELETE /api/schedules/:id should delete a schedule', async () => {
    (pool.query as any).mockResolvedValueOnce({
      rows: [{ source: 'api' }],
    } as unknown as QueryResult<any>);
    vi.mocked(mockScheduleService.deleteSchedule).mockResolvedValueOnce(undefined);

    const res = await request(app).delete('/api/schedules/123');
    expect(res.status).toBe(204);
    expect(mockScheduleService.deleteSchedule).toHaveBeenCalledWith('123');
  });
});
