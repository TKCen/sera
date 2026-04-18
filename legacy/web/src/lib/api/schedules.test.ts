import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  listSchedules,
  createSchedule,
  updateSchedule,
  deleteSchedule,
  triggerSchedule,
  listScheduleRuns,
  getScheduleRuns,
} from './schedules';
import { request } from './client';

vi.mock('./client', () => ({
  request: vi.fn(),
}));

describe('schedules api', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('listSchedules', () => {
    it('should call request with correct URL when no params are provided', async () => {
      vi.mocked(request).mockResolvedValueOnce([]);
      await listSchedules();
      expect(request).toHaveBeenCalledWith('/schedules');
    });

    it('should call request with correct query parameters', async () => {
      vi.mocked(request).mockResolvedValueOnce([]);
      await listSchedules({ agentName: 'test-agent', status: 'active' });
      expect(request).toHaveBeenCalledWith('/schedules?agentName=test-agent&status=active');
    });
  });

  describe('createSchedule', () => {
    it('should call request with correct parameters', async () => {
      const scheduleData = {
        agentName: 'test-agent',
        name: 'test-schedule',
        type: 'cron' as const,
        expression: '0 * * * *',
        taskPrompt: 'do something',
        status: 'active' as const,
      };
      const mockResponse = { id: '123', ...scheduleData, source: 'api' };
      vi.mocked(request).mockResolvedValueOnce(mockResponse);

      const result = await createSchedule(scheduleData);

      expect(request).toHaveBeenCalledWith('/schedules', {
        method: 'POST',
        body: JSON.stringify(scheduleData),
      });
      expect(result).toEqual(mockResponse);
    });
  });

  describe('updateSchedule', () => {
    it('should call request with correct parameters and map taskPrompt to task', async () => {
      const id = '123';
      const updateData = {
        name: 'updated-name',
        taskPrompt: 'new task',
      };
      vi.mocked(request).mockResolvedValueOnce({});

      await updateSchedule(id, updateData);

      expect(request).toHaveBeenCalledWith(`/schedules/${id}`, {
        method: 'PATCH',
        body: JSON.stringify({ name: 'updated-name', task: 'new task' }),
      });
    });

    it('should handle ID encoding', async () => {
      const id = 'test schedule';
      vi.mocked(request).mockResolvedValueOnce({});

      await updateSchedule(id, { name: 'new name' });

      expect(request).toHaveBeenCalledWith(`/schedules/test%20schedule`, {
        method: 'PATCH',
        body: JSON.stringify({ name: 'new name' }),
      });
    });
  });

  describe('deleteSchedule', () => {
    it('should call request with DELETE method and encoded ID', async () => {
      const id = 'test/schedule';
      vi.mocked(request).mockResolvedValueOnce({ success: true });

      const result = await deleteSchedule(id);

      expect(request).toHaveBeenCalledWith(`/schedules/test%2Fschedule`, {
        method: 'DELETE',
      });
      expect(result).toEqual({ success: true });
    });
  });

  describe('triggerSchedule', () => {
    it('should call request with POST method and no query param by default', async () => {
      const id = '123';
      vi.mocked(request).mockResolvedValueOnce({ status: 'triggered' });

      await triggerSchedule(id);

      expect(request).toHaveBeenCalledWith(`/schedules/${id}/trigger`, {
        method: 'POST',
      });
    });

    it('should call request with force query param when force is true', async () => {
      const id = '123';
      vi.mocked(request).mockResolvedValueOnce({ status: 'triggered' });

      await triggerSchedule(id, true);

      expect(request).toHaveBeenCalledWith(`/schedules/${id}/trigger?force=true`, {
        method: 'POST',
      });
    });

    it('should handle ID encoding', async () => {
      const id = 'test schedule';
      vi.mocked(request).mockResolvedValueOnce({ status: 'triggered' });

      await triggerSchedule(id);

      expect(request).toHaveBeenCalledWith(`/schedules/test%20schedule/trigger`, {
        method: 'POST',
      });
    });
  });

  describe('listScheduleRuns', () => {
    it('should call request with correct URL when no params are provided', async () => {
      vi.mocked(request).mockResolvedValueOnce([]);
      await listScheduleRuns();
      expect(request).toHaveBeenCalledWith('/schedules/runs');
    });

    it('should call request with correct query parameters', async () => {
      vi.mocked(request).mockResolvedValueOnce([]);
      const params = {
        category: 'test-cat',
        scheduleId: 'sched-123',
        agentId: 'agent-456',
        limit: 10,
      };
      await listScheduleRuns(params);
      expect(request).toHaveBeenCalledWith(
        '/schedules/runs?category=test-cat&scheduleId=sched-123&agentId=agent-456&limit=10'
      );
    });
  });

  describe('getScheduleRuns', () => {
    it('should call request with correct URL and encoded ID', async () => {
      const id = 'test schedule';
      vi.mocked(request).mockResolvedValueOnce([]);
      await getScheduleRuns(id);
      expect(request).toHaveBeenCalledWith('/schedules/test%20schedule/runs');
    });

    it('should call request with limit parameter', async () => {
      const id = '123';
      vi.mocked(request).mockResolvedValueOnce([]);
      await getScheduleRuns(id, { limit: 5 });
      expect(request).toHaveBeenCalledWith('/schedules/123/runs?limit=5');
    });
  });
});
