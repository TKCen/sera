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
import type { Schedule } from './types';

vi.mock('./client', () => ({
  request: vi.fn(),
}));

describe('schedules api', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('listSchedules', () => {
    it('should call request with /schedules when no params provided', async () => {
      await listSchedules();
      expect(request).toHaveBeenCalledWith('/schedules');
    });

    it('should include agentName and status in query string', async () => {
      await listSchedules({ agentName: 'my-agent', status: 'active' });
      expect(request).toHaveBeenCalledWith('/schedules?agentName=my-agent&status=active');
    });
  });

  describe('createSchedule', () => {
    it('should call request with POST and correct body', async () => {
      const data: Omit<
        Schedule,
        'id' | 'source' | 'lastRunAt' | 'lastRunStatus' | 'lastRunOutput' | 'nextRunAt'
      > = {
        agentName: 'agent-1',
        name: 'test-schedule',
        type: 'cron',
        expression: '* * * * *',
        taskPrompt: 'do something',
        status: 'active',
      };
      await createSchedule(data);
      expect(request).toHaveBeenCalledWith('/schedules', {
        method: 'POST',
        body: JSON.stringify(data),
      });
    });
  });

  describe('updateSchedule', () => {
    it('should map taskPrompt to task in payload', async () => {
      await updateSchedule('sched-1', { taskPrompt: 'new prompt', name: 'new name' });
      expect(request).toHaveBeenCalledWith('/schedules/sched-1', {
        method: 'PATCH',
        body: JSON.stringify({ name: 'new name', task: 'new prompt' }),
      });
    });

    it('should not include task if taskPrompt is undefined', async () => {
      await updateSchedule('sched-1', { name: 'new name' });
      expect(request).toHaveBeenCalledWith('/schedules/sched-1', {
        method: 'PATCH',
        body: JSON.stringify({ name: 'new name' }),
      });
    });
  });

  describe('deleteSchedule', () => {
    it('should call request with DELETE and encoded id', async () => {
      await deleteSchedule('sched 1');
      expect(request).toHaveBeenCalledWith('/schedules/sched%201', {
        method: 'DELETE',
      });
    });
  });

  describe('triggerSchedule', () => {
    it('should call request with POST and no force param by default', async () => {
      await triggerSchedule('sched-1');
      expect(request).toHaveBeenCalledWith('/schedules/sched-1/trigger', {
        method: 'POST',
      });
    });

    it('should include force=true in query string when force is true', async () => {
      await triggerSchedule('sched-1', true);
      expect(request).toHaveBeenCalledWith('/schedules/sched-1/trigger?force=true', {
        method: 'POST',
      });
    });
  });

  describe('listScheduleRuns', () => {
    it('should include all filters in query string', async () => {
      await listScheduleRuns({
        category: 'cat1',
        scheduleId: 's1',
        agentId: 'a1',
        limit: 10,
      });
      expect(request).toHaveBeenCalledWith(
        '/schedules/runs?category=cat1&scheduleId=s1&agentId=a1&limit=10'
      );
    });
  });

  describe('getScheduleRuns', () => {
    it('should include limit in query string and encoded id', async () => {
      await getScheduleRuns('sched 1', { limit: 5 });
      expect(request).toHaveBeenCalledWith('/schedules/sched%201/runs?limit=5');
    });
  });
});
