import { describe, it, expect, vi, beforeEach } from 'vitest';
import { scheduleTaskSkill } from './schedule-task.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';

// Mock database
vi.mock('../../lib/database.js', () => ({
  query: vi.fn(),
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
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 1 } as never);

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
      expect(mockQuery).toHaveBeenCalledWith(
        expect.stringContaining("status = 'active'"),
        expect.arrayContaining(['sched-1'])
      );
    });
  });

  describe('deactivate action', () => {
    it('sets status to paused', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 1 } as never);

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
      expect(mockQuery).toHaveBeenCalledWith(
        expect.stringContaining("status = 'paused'"),
        expect.arrayContaining(['sched-1'])
      );
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
      // Delete query
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 1 } as never);

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
