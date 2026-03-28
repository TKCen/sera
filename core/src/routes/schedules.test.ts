import { describe, it, expect, vi, beforeEach } from 'vitest';
import express from 'express';
import request from 'supertest';
import { createSchedulesRouter } from './schedules.js';
import { ScheduleService } from '../services/ScheduleService.js';
import { pool } from '../lib/database.js';

vi.mock('../services/ScheduleService.js', () => {
  const mockScheduleService = {
    createSchedule: vi.fn(),
    updateSchedule: vi.fn(),
    deleteSchedule: vi.fn(),
    triggerSchedule: vi.fn(),
  };

  return {
    ScheduleService: {
      getInstance: vi.fn(() => mockScheduleService),
    },
  };
});

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
  },
}));

describe('SchedulesRouter', () => {
  let app: express.Express;
  let mockScheduleService: any;

  beforeEach(() => {
    vi.clearAllMocks();

    mockScheduleService = ScheduleService.getInstance();

    app = express();
    app.use(express.json());
    app.use('/api/schedules', createSchedulesRouter());
  });

  describe('GET /api/schedules', () => {
    it('returns a list of schedules mapped to camelCase', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [
          {
            id: 'sched-1',
            resolved_agent_name: 'Agent 1',
            agent_instance_id: 'agent-1-id',
            name: 'Daily task',
            type: 'cron',
            cron_expression: '0 0 * * *',
            task_prompt: 'Do something',
            status: 'active',
            source: 'api',
            last_run_at: new Date('2023-10-01T00:00:00Z'),
            last_run_status: 'success',
            last_run_output: 'Done',
            next_run_at: new Date('2023-10-02T00:00:00Z'),
            created_at: new Date('2023-09-01T00:00:00Z'),
            updated_at: new Date('2023-09-01T00:00:00Z'),
          },
        ],
        command: 'SELECT',
        rowCount: 1,
        oid: 0,
        fields: [],
      } as any);

      const res = await request(app).get('/api/schedules');

      expect(res.status).toBe(200);
      expect(res.body).toEqual([
        {
          id: 'sched-1',
          agentName: 'Agent 1',
          agentInstanceId: 'agent-1-id',
          name: 'Daily task',
          type: 'cron',
          expression: '0 0 * * *',
          taskPrompt: 'Do something',
          status: 'active',
          source: 'api',
          lastRunAt: '2023-10-01T00:00:00.000Z',
          lastRunStatus: 'success',
          lastRunOutput: 'Done',
          nextRunAt: '2023-10-02T00:00:00.000Z',
          createdAt: '2023-09-01T00:00:00.000Z',
          updatedAt: '2023-09-01T00:00:00.000Z',
        },
      ]);
      expect(pool.query).toHaveBeenCalledWith(
        expect.stringContaining('SELECT s.*, ai.name AS resolved_agent_name'),
        []
      );
    });

    it('filters schedules by agentId', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [],
        command: 'SELECT',
        rowCount: 0,
        oid: 0,
        fields: [],
      } as any);

      const res = await request(app).get('/api/schedules?agentId=agent-1-id');

      expect(res.status).toBe(200);
      expect(res.body).toEqual([]);
      expect(pool.query).toHaveBeenCalledWith(
        expect.stringContaining('WHERE s.agent_instance_id = $1'),
        ['agent-1-id']
      );
    });

    it('returns 500 on database error', async () => {
      vi.mocked(pool.query).mockRejectedValueOnce(new Error('DB error'));

      const res = await request(app).get('/api/schedules');

      expect(res.status).toBe(500);
      expect(res.body).toEqual({ error: 'DB error' });
    });
  });

  describe('POST /api/schedules', () => {
    it('creates a schedule and returns it', async () => {
      const scheduleInput = {
        agentInstanceId: 'agent-1-id',
        name: 'New schedule',
        expression: '0 * * * *',
        taskPrompt: 'Prompt',
      };
      const createdSchedule = { id: 'sched-2', ...scheduleInput };

      mockScheduleService.createSchedule.mockResolvedValueOnce(createdSchedule);

      const res = await request(app).post('/api/schedules').send(scheduleInput);

      expect(res.status).toBe(201);
      expect(res.body).toEqual(createdSchedule);
      expect(mockScheduleService.createSchedule).toHaveBeenCalledWith(scheduleInput);
    });

    it('returns 400 on service error', async () => {
      mockScheduleService.createSchedule.mockRejectedValueOnce(new Error('Invalid expression'));

      const res = await request(app).post('/api/schedules').send({});

      expect(res.status).toBe(400);
      expect(res.body).toEqual({ error: 'Invalid expression' });
    });
  });

  describe('GET /api/schedules/:id', () => {
    it('returns a specific schedule', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [{ id: 'sched-1', name: 'Task' }],
        command: 'SELECT',
        rowCount: 1,
        oid: 0,
        fields: [],
      } as any);

      const res = await request(app).get('/api/schedules/sched-1');

      expect(res.status).toBe(200);
      expect(res.body).toEqual({ id: 'sched-1', name: 'Task' });
      expect(pool.query).toHaveBeenCalledWith('SELECT * FROM schedules WHERE id = $1', ['sched-1']);
    });

    it('returns 404 if schedule not found', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [],
        command: 'SELECT',
        rowCount: 0,
        oid: 0,
        fields: [],
      } as any);

      const res = await request(app).get('/api/schedules/sched-unknown');

      expect(res.status).toBe(404);
      expect(res.body).toEqual({ error: 'Schedule not found' });
    });

    it('returns 500 on database error', async () => {
      vi.mocked(pool.query).mockRejectedValueOnce(new Error('DB error'));

      const res = await request(app).get('/api/schedules/sched-1');

      expect(res.status).toBe(500);
      expect(res.body).toEqual({ error: 'DB error' });
    });
  });

  describe('PATCH /api/schedules/:id', () => {
    it('updates a schedule and returns it', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [{ source: 'api' }],
        command: 'SELECT',
        rowCount: 1,
        oid: 0,
        fields: [],
      } as any);

      const updatedSchedule = { id: 'sched-1', name: 'Updated Task' };
      mockScheduleService.updateSchedule.mockResolvedValueOnce(updatedSchedule);

      const res = await request(app).patch('/api/schedules/sched-1').send({ name: 'Updated Task' });

      expect(res.status).toBe(200);
      expect(res.body).toEqual(updatedSchedule);
      expect(pool.query).toHaveBeenCalledWith('SELECT source FROM schedules WHERE id = $1', [
        'sched-1',
      ]);
      expect(mockScheduleService.updateSchedule).toHaveBeenCalledWith('sched-1', {
        name: 'Updated Task',
      });
    });

    it('returns 404 if schedule to update is not found', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [],
        command: 'SELECT',
        rowCount: 0,
        oid: 0,
        fields: [],
      } as any);

      const res = await request(app).patch('/api/schedules/sched-1').send({ name: 'Task' });

      expect(res.status).toBe(404);
      expect(res.body).toEqual({ error: 'Schedule not found' });
      expect(mockScheduleService.updateSchedule).not.toHaveBeenCalled();
    });

    it('returns 400 on service update error', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [{ source: 'api' }],
        command: 'SELECT',
        rowCount: 1,
        oid: 0,
        fields: [],
      } as any);

      mockScheduleService.updateSchedule.mockRejectedValueOnce(new Error('Invalid update'));

      const res = await request(app).patch('/api/schedules/sched-1').send({ name: 'Task' });

      expect(res.status).toBe(400);
      expect(res.body).toEqual({ error: 'Invalid update' });
    });
  });

  describe('DELETE /api/schedules/:id', () => {
    it('deletes an API-created schedule', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [{ source: 'api' }],
        command: 'SELECT',
        rowCount: 1,
        oid: 0,
        fields: [],
      } as any);

      mockScheduleService.deleteSchedule.mockResolvedValueOnce(undefined);

      const res = await request(app).delete('/api/schedules/sched-1');

      expect(res.status).toBe(204);
      expect(pool.query).toHaveBeenCalledWith('SELECT source FROM schedules WHERE id = $1', [
        'sched-1',
      ]);
      expect(mockScheduleService.deleteSchedule).toHaveBeenCalledWith('sched-1');
    });

    it('returns 403 if schedule source is manifest', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [{ source: 'manifest' }],
        command: 'SELECT',
        rowCount: 1,
        oid: 0,
        fields: [],
      } as any);

      const res = await request(app).delete('/api/schedules/sched-1');

      expect(res.status).toBe(403);
      expect(res.body.error).toContain('Manifest-declared schedules cannot be deleted via API');
      expect(mockScheduleService.deleteSchedule).not.toHaveBeenCalled();
    });

    it('returns 404 if schedule not found', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [],
        command: 'SELECT',
        rowCount: 0,
        oid: 0,
        fields: [],
      } as any);

      const res = await request(app).delete('/api/schedules/sched-1');

      expect(res.status).toBe(404);
      expect(res.body).toEqual({ error: 'Schedule not found' });
      expect(mockScheduleService.deleteSchedule).not.toHaveBeenCalled();
    });

    it('returns 500 on database error', async () => {
      vi.mocked(pool.query).mockRejectedValueOnce(new Error('DB error'));

      const res = await request(app).delete('/api/schedules/sched-1');

      expect(res.status).toBe(500);
      expect(res.body).toEqual({ error: 'DB error' });
    });
  });

  describe('POST /api/schedules/:id/trigger', () => {
    it('manually triggers a schedule', async () => {
      mockScheduleService.triggerSchedule.mockResolvedValueOnce(undefined);

      const res = await request(app).post('/api/schedules/sched-1/trigger');

      expect(res.status).toBe(200);
      expect(res.body).toEqual({ status: 'triggered' });
      expect(mockScheduleService.triggerSchedule).toHaveBeenCalledWith('sched-1');
    });

    it('returns 500 if triggering fails', async () => {
      mockScheduleService.triggerSchedule.mockRejectedValueOnce(new Error('Trigger error'));

      const res = await request(app).post('/api/schedules/sched-1/trigger');

      expect(res.status).toBe(500);
      expect(res.body).toEqual({ error: 'Trigger error' });
    });
  });
});
