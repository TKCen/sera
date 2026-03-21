import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ScheduleService } from './ScheduleService.js';
import type { PgBoss } from 'pg-boss';
import { pool } from '../lib/database.js';

// Mock Database
vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
    connect: vi.fn().mockResolvedValue({
      query: vi.fn(),
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
  PgBossMock.prototype.createQueue = vi.fn().mockResolvedValue(undefined);
  return { PgBoss: PgBossMock };
});

// Mock AuditService
vi.mock('../audit/AuditService.js', () => ({
  AuditService: {
    getInstance: () => ({
      record: vi.fn().mockResolvedValue(undefined),
    }),
  },
}));

describe('ScheduleService', () => {
  let service: ScheduleService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = ScheduleService.getInstance();
    (service as unknown as { initialized: boolean }).initialized = false;
  });

  describe('reconcile', () => {
    it('adds missing schedules to pg-boss and removes stale ones', async () => {
      // Setup: 1 active schedule in DB, 1 stale schedule in pg-boss
      const mockDbSchedule = {
        id: '11111111-1111-4111-a111-111111111111',
        name: 'test-job',
        expression: '* * * * *',
        status: 'active',
        type: 'cron',
      };

      (pool.query as unknown as import('vitest').Mock)
        .mockResolvedValueOnce({ rows: [mockDbSchedule] }) // reconcile in start()
        .mockResolvedValueOnce({ rows: [] }) // pgboss query in start()
        .mockResolvedValueOnce({ rows: [mockDbSchedule] }) // manual reconcile call
        .mockResolvedValueOnce({ rows: [{ name: '22222222-2222-4222-a222-222222222222' }] }); // stale job

      await service.start({ schedule: vi.fn(), unschedule: vi.fn(), createQueue: vi.fn(), work: vi.fn() } as unknown as PgBoss);
      await service.reconcile();

      const boss = (
        service as unknown as {
          boss: { schedule: import('vitest').Mock; unschedule: import('vitest').Mock };
        }
      ).boss;

      // Verify missing schedule was added
      expect(boss.schedule).toHaveBeenCalledWith(mockDbSchedule.id, mockDbSchedule.expression, {
        scheduleId: mockDbSchedule.id,
      });

      // Verify stale schedule was removed
      expect(boss.unschedule).toHaveBeenCalledWith('22222222-2222-4222-a222-222222222222');
    });
  });
});
