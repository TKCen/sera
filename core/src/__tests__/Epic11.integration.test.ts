import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { PoolClient } from 'pg';
import { ScheduleService } from '../services/ScheduleService.js';
import { AuditService } from '../audit/AuditService.js';
import { pool } from '../lib/database.js';
import type { Orchestrator } from '../agents/Orchestrator.js';

// Mock Database Pool
vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
    connect: vi.fn().mockResolvedValue({
      query: vi.fn().mockResolvedValue({ rows: [] }),
      release: vi.fn(),
    }),
  },
}));

// Mock pg-boss
vi.mock('pg-boss', () => {
  const PgBossMock = vi.fn();
  PgBossMock.prototype.start = vi.fn().mockResolvedValue(undefined);
  PgBossMock.prototype.stop = vi.fn().mockResolvedValue(undefined);
  PgBossMock.prototype.schedule = vi.fn().mockResolvedValue(undefined);
  PgBossMock.prototype.unschedule = vi.fn().mockResolvedValue(undefined);
  PgBossMock.prototype.work = vi.fn().mockResolvedValue(undefined);
  return { PgBoss: PgBossMock };
});

describe('Epic 11 Integration', () => {
  let scheduleService: ScheduleService;
  let auditService: AuditService;

  beforeEach(() => {
    vi.clearAllMocks();
    scheduleService = ScheduleService.getInstance();
    auditService = AuditService.getInstance();
    (scheduleService as unknown as { initialized: boolean }).initialized = false;
    (auditService as unknown as { initialized: boolean }).initialized = false;
  });

  it('firing a scheduled task creates an audit record', async () => {
    const mockOrchestrator = {
      startInstance: vi.fn().mockResolvedValue({}),
    };
    scheduleService.setOrchestrator(mockOrchestrator as unknown as Orchestrator);

    const scheduleId = '11111111-1111-4111-a111-111111111111';
    const agentId = '22222222-2222-4222-a222-222222222222';

    // Mock schedule lookup
    vi.mocked(pool.query).mockImplementation(((q: string, _params: unknown[] | undefined) => {
      if (q.includes('FROM schedules')) {
        return Promise.resolve({
          rows: [
            {
              id: scheduleId,
              agent_instance_id: agentId,
              agent_name: 'test-agent',
              name: 'test-schedule',
              task: 'do something',
              type: 'cron',
              status: 'active',
            },
          ],
        });
      }
      if (q.includes('FROM agent_instances')) {
        return Promise.resolve({
          rows: [{ lifecycle_mode: 'ephemeral', status: 'stopped' }],
        });
      }
      return Promise.resolve({ rows: [] });
    }) as unknown as typeof pool.query);

    const clientMock = {
      query: vi.fn().mockResolvedValue({ rows: [{ seq: '100', hash: 'some-hash' }] }),
      release: vi.fn(),
    };
    vi.mocked(pool.connect).mockResolvedValue(clientMock as any);

    // Trigger the schedule
    await scheduleService.triggerSchedule(scheduleId);

    // Verify orchestrator was called
    expect(mockOrchestrator.startInstance).toHaveBeenCalledWith(agentId, undefined, 'do something');

    // Verify audit record was created
    const recordCall = clientMock.query.mock.calls.find((c) =>
      (c[0] as string).includes('INSERT INTO audit_trail')
    );
    expect(recordCall).toBeDefined();
    if (recordCall) {
      expect(recordCall[1]).toContain('schedule.fired');
      const payload = (recordCall[1] as unknown as unknown[])[6] as { scheduleId: string };
      expect(payload.scheduleId).toBe(scheduleId);
    }
  });
});
