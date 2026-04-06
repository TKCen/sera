import { describe, it, expect, vi, beforeEach } from 'vitest';
import { scheduleTaskSkill } from './schedule-task.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';

// Mock database
vi.mock('../../lib/database.js', () => ({
  query: vi.fn(),
}));

// Mock ScheduleService so tests don't need pg-boss or a real DB pool
const mockUpdateSchedule = vi.fn();
const mockDeleteSchedule = vi.fn();
const mockCreateSchedule = vi.fn();
vi.mock('../../services/ScheduleService.js', () => ({
  ScheduleService: {
    getInstance: () => ({
      updateSchedule: mockUpdateSchedule,
      deleteSchedule: mockDeleteSchedule,
      createSchedule: mockCreateSchedule,
    }),
  },
}));

import { query } from '../../lib/database.js';
const mockQuery = vi.mocked(query);

const mockContext: AgentContext = {
  agentName: 'TestAgent',
  workspacePath: '/tmp/test',
  tier: 1 as SecurityTier,
  manifest: {
    apiVersion: 'v1',
    kind: 'Agent',
    metadata: {
      name: 'TestAgent',
      displayName: 'Test Agent',
      icon: '',
      circle: 'test',
      tier: 1 as SecurityTier,
    },
    identity: { role: 'tester', description: 'Test agent' },
    model: { provider: 'openai', name: 'gpt-4' },
  },
  agentInstanceId: 'agent-001',
  containerId: 'container-001',
  sandboxManager: {} as never,
  sessionId: 'session-001',
};

describe('schedule-task skill', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUpdateSchedule.mockResolvedValue({ id: 'sched-1', status: 'active' });
    mockDeleteSchedule.mockResolvedValue(undefined);
    mockCreateSchedule.mockResolvedValue({ id: 'sched-new' });
  });

  describe('get action', () => {
    it('returns schedule details', async () => {
      const row = {
        id: 'sched-1',
        name: 'Daily Report',
        cron: '0 9 * * *',
        status: 'active',
        source: 'api',
        description: 'A daily report',
      };
      mockQuery.mockResolvedValueOnce({ rows: [row], rowCount: 1 } as never);

      const result = await scheduleTaskSkill.handler(
        { action: 'get', scheduleId: 'sched-1' },
        mockContext
      );
      expect(result).toEqual(
        expect.objectContaining({
          success: true,
          data: expect.objectContaining({ schedule: row }),
        })
      );
    });

    it('requires scheduleId', async () => {
      const result = await scheduleTaskSkill.handler({ action: 'get' }, mockContext);
      expect(result).toEqual(
        expect.objectContaining({
          success: false,
          error: expect.stringContaining('scheduleId'),
        })
      );
    });
  });

  describe('activate action', () => {
    it('sets status to active', async () => {
      // Ownership check query
      mockQuery.mockResolvedValueOnce({ rows: [{ id: 'sched-1' }], rowCount: 1 } as never);
      mockUpdateSchedule.mockResolvedValueOnce({ id: 'sched-1', status: 'active' });

      const result = await scheduleTaskSkill.handler(
        { action: 'activate', scheduleId: 'sched-1' },
        mockContext
      );
      expect(result).toEqual(
        expect.objectContaining({
          success: true,
          data: expect.objectContaining({ message: expect.stringContaining('activated') }),
        })
      );
      expect(mockUpdateSchedule).toHaveBeenCalledWith('sched-1', { status: 'active' });
    });
  });

  describe('deactivate action', () => {
    it('sets status to paused', async () => {
      // Ownership check query
      mockQuery.mockResolvedValueOnce({ rows: [{ id: 'sched-1' }], rowCount: 1 } as never);
      mockUpdateSchedule.mockResolvedValueOnce({ id: 'sched-1', status: 'paused' });

      const result = await scheduleTaskSkill.handler(
        { action: 'deactivate', scheduleId: 'sched-1' },
        mockContext
      );
      expect(result).toEqual(
        expect.objectContaining({
          success: true,
          data: expect.objectContaining({ message: expect.stringContaining('paused') }),
        })
      );
      expect(mockUpdateSchedule).toHaveBeenCalledWith('sched-1', { status: 'paused' });
    });
  });

  describe('delete action — manifest protection', () => {
    it('rejects deletion of manifest schedules', async () => {
      // Source check query
      mockQuery.mockResolvedValueOnce({
        rows: [{ source: 'manifest' }],
        rowCount: 1,
      } as never);

      const result = await scheduleTaskSkill.handler(
        { action: 'delete', scheduleId: 'sched-manifest' },
        mockContext
      );
      expect(result).toEqual(
        expect.objectContaining({
          success: false,
          error: expect.stringContaining('manifest'),
        })
      );
    });

    it('allows deletion of api schedules', async () => {
      // Source check query
      mockQuery.mockResolvedValueOnce({
        rows: [{ source: 'api' }],
        rowCount: 1,
      } as never);
      mockDeleteSchedule.mockResolvedValueOnce(undefined);

      const result = await scheduleTaskSkill.handler(
        { action: 'delete', scheduleId: 'sched-api' },
        mockContext
      );
      expect(result).toEqual(
        expect.objectContaining({
          success: true,
          data: expect.objectContaining({ message: expect.stringContaining('deleted') }),
        })
      );
      expect(mockDeleteSchedule).toHaveBeenCalledWith('sched-api');
    });
  });

  describe('list action', () => {
    it('includes source and description fields', async () => {
      const rows = [
        {
          id: 'sched-1',
          name: 'Report',
          cron: '0 9 * * *',
          status: 'active',
          source: 'manifest',
          description: 'Daily report',
          category: 'general',
        },
      ];
      mockQuery.mockResolvedValueOnce({ rows, rowCount: 1 } as never);

      const result = await scheduleTaskSkill.handler({ action: 'list' }, mockContext);
      expect(result).toEqual(
        expect.objectContaining({
          success: true,
          data: expect.objectContaining({
            schedules: expect.arrayContaining([
              expect.objectContaining({ source: 'manifest', description: 'Daily report' }),
            ]),
          }),
        })
      );
      // Verify the SQL includes source and description
      const sqlArg = mockQuery.mock.calls[0]![0] as string;
      expect(sqlArg).toContain('source');
      expect(sqlArg).toContain('description');
    });
  });
});
