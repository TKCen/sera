import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  listSchedules,
  createSchedule,
  updateSchedule,
  deleteSchedule,
  triggerSchedule,
  listScheduleRuns,
  getScheduleRuns,
} from './schedules';

describe('schedules API', () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    vi.stubGlobal('fetch', mockFetch);
    mockFetch.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({}),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  describe('listSchedules', () => {
    it('calls GET /schedules without params', async () => {
      await listSchedules();
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/schedules$/),
        expect.any(Object)
      );
    });

    it('calls GET /schedules with agentName and status', async () => {
      await listSchedules({ agentName: 'my-agent', status: 'active' });
      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain('agentName=my-agent');
      expect(url).toContain('status=active');
      expect(url).toContain('/api/schedules?');
    });
  });

  describe('createSchedule', () => {
    it('calls POST /schedules with data', async () => {
      const data = {
        agentName: 'my-agent',
        name: 'test-schedule',
        type: 'cron' as const,
        expression: '0 * * * *',
        status: 'active' as const,
      };
      await createSchedule(data as any);
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/schedules$/),
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify(data),
        })
      );
    });
  });

  describe('updateSchedule', () => {
    it('calls PATCH /schedules/:id and maps taskPrompt to task', async () => {
      const id = 'sched-123';
      const data = {
        name: 'updated-name',
        taskPrompt: 'new task',
      };
      await updateSchedule(id, data);

      const expectedPayload = {
        name: 'updated-name',
        task: 'new task',
      };

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/schedules\/sched-123$/),
        expect.objectContaining({
          method: 'PATCH',
          body: JSON.stringify(expectedPayload),
        })
      );
    });

    it('calls PATCH /schedules/:id without taskPrompt mapping if not provided', async () => {
      const id = 'sched-123';
      const data = {
        status: 'paused' as const,
      };
      await updateSchedule(id, data);

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/schedules\/sched-123$/),
        expect.objectContaining({
          method: 'PATCH',
          body: JSON.stringify({ status: 'paused' }),
        })
      );
    });
  });

  describe('deleteSchedule', () => {
    it('calls DELETE /schedules/:id', async () => {
      const id = 'sched-123';
      await deleteSchedule(id);
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/schedules\/sched-123$/),
        expect.objectContaining({ method: 'DELETE' })
      );
    });
  });

  describe('triggerSchedule', () => {
    it('calls POST /schedules/:id/trigger without force', async () => {
      const id = 'sched-123';
      await triggerSchedule(id);
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/schedules\/sched-123\/trigger$/),
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('calls POST /schedules/:id/trigger with force=true', async () => {
      const id = 'sched-123';
      await triggerSchedule(id, true);
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/schedules\/sched-123\/trigger\?force=true$/),
        expect.objectContaining({ method: 'POST' })
      );
    });
  });

  describe('listScheduleRuns', () => {
    it('calls GET /schedules/runs with params', async () => {
      await listScheduleRuns({ category: 'test', limit: 10 });
      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain('/api/schedules/runs?');
      expect(url).toContain('category=test');
      expect(url).toContain('limit=10');
    });
  });

  describe('getScheduleRuns', () => {
    it('calls GET /schedules/:id/runs with limit', async () => {
      const id = 'sched-123';
      await getScheduleRuns(id, { limit: 5 });
      const url = mockFetch.mock.calls[0][0];
      expect(url).toContain(`/api/schedules/${id}/runs?`);
      expect(url).toContain('limit=5');
    });
  });
});
