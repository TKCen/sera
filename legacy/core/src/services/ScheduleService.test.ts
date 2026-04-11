import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest';
import { ScheduleService } from './ScheduleService.js';
import { pool } from '../lib/database.js';
import { PgBoss } from 'pg-boss';

const mockQuery = pool.query as unknown as Mock;

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
        .mockResolvedValueOnce({ rows: [mockDbSchedule] }) // reconcile in start(): active schedules
        .mockResolvedValueOnce({ rows: [] }) // reconcile in start(): UPDATE next_run_at
        .mockResolvedValueOnce({ rows: [] }) // reconcile in start(): pgboss stale query
        .mockResolvedValueOnce({ rows: [mockDbSchedule] }) // manual reconcile: active schedules
        .mockResolvedValueOnce({ rows: [] }) // manual reconcile: UPDATE next_run_at
        .mockResolvedValueOnce({ rows: [{ name: '22222222-2222-4222-a222-222222222222' }] }); // stale job

      const boss = new PgBoss('postgres://localhost/test');
      await service.start(boss);
      await service.reconcile();

      const serviceBoss = (
        service as unknown as {
          boss: { schedule: import('vitest').Mock; unschedule: import('vitest').Mock };
        }
      ).boss;

      // Verify missing schedule was added
      expect(serviceBoss.schedule).toHaveBeenCalledWith(
        mockDbSchedule.id,
        mockDbSchedule.expression,
        {
          scheduleId: mockDbSchedule.id,
        }
      );

      // Verify stale schedule was removed
      expect(serviceBoss.unschedule).toHaveBeenCalledWith('22222222-2222-4222-a222-222222222222');
    });
  });

  describe('createSchedule', () => {
    it('rejects invalid cron expressions', async () => {
      await expect(
        service.createSchedule({
          agent_instance_id: '11111111-1111-4111-a111-111111111111',
          agent_name: 'test-agent',
          name: 'bad-cron',
          type: 'cron',
          expression: 'not-valid',
          task: 'do something',
        })
      ).rejects.toThrow('Invalid cron expression');
    });

    it('accepts valid cron expressions and includes next_run_at', async () => {
      const mockSchedule = {
        id: '33333333-3333-4333-a333-333333333333',
        name: 'good-cron',
        expression: '0 */8 * * *',
        type: 'cron',
        status: 'active',
        agent_instance_id: '11111111-1111-4111-a111-111111111111',
      };

      mockQuery.mockResolvedValueOnce({ rows: [mockSchedule] }); // INSERT
      // AuditService.record is already mocked

      const result = await service.createSchedule({
        agent_instance_id: '11111111-1111-4111-a111-111111111111',
        agent_name: 'test-agent',
        name: 'good-cron',
        type: 'cron',
        expression: '0 */8 * * *',
        task: 'do something',
      });

      expect(result.name).toBe('good-cron');
      // Verify INSERT included description and next_run_at (12 params)
      const insertCall = mockQuery.mock.calls[0]!;
      expect(insertCall[1]).toHaveLength(12);
    });

    it('accepts once-type schedules without cron validation', async () => {
      const mockSchedule = {
        id: '44444444-4444-4444-a444-444444444444',
        name: 'one-shot',
        expression: '2026-05-01T00:00:00Z',
        type: 'once',
        status: 'active',
      };

      mockQuery.mockResolvedValueOnce({ rows: [mockSchedule] });

      const result = await service.createSchedule({
        agent_instance_id: '11111111-1111-4111-a111-111111111111',
        agent_name: 'test-agent',
        name: 'one-shot',
        type: 'once',
        expression: '2026-05-01T00:00:00Z',
        task: 'do something once',
      });

      expect(result.name).toBe('one-shot');
    });
  });

  describe('upsertManifestSchedule', () => {
    it('inserts a new manifest schedule', async () => {
      const mockSchedule = {
        id: '55555555-5555-4555-a555-555555555555',
        name: 'Reflection',
        expression: '0 */8 * * *',
        type: 'cron',
        status: 'paused',
        source: 'manifest',
        agent_instance_id: '11111111-1111-4111-a111-111111111111',
      };

      mockQuery.mockResolvedValueOnce({ rows: [mockSchedule] });

      const result = await service.upsertManifestSchedule({
        agent_instance_id: '11111111-1111-4111-a111-111111111111',
        agent_name: 'sera',
        name: 'Reflection',
        type: 'cron',
        expression: '0 */8 * * *',
        task: 'Reflect on recent interactions',
        status: 'paused',
        category: 'reflection',
      });

      expect(result.name).toBe('Reflection');
      expect(result.source).toBe('manifest');
    });

    it('does not overwrite API-created schedules', async () => {
      const existingApiSchedule = {
        id: '66666666-6666-4666-a666-666666666666',
        name: 'custom-schedule',
        source: 'api',
        status: 'active',
      };

      // ON CONFLICT with WHERE source='manifest' returns no rows when source='api'
      mockQuery
        .mockResolvedValueOnce({ rows: [] }) // INSERT returns empty (conflict, api source)
        .mockResolvedValueOnce({ rows: [existingApiSchedule] }); // fallback SELECT

      const result = await service.upsertManifestSchedule({
        agent_instance_id: '11111111-1111-4111-a111-111111111111',
        agent_name: 'sera',
        name: 'custom-schedule',
        type: 'cron',
        expression: '0 0 * * *',
        task: 'overwrite attempt',
        status: 'active',
      });

      expect(result.source).toBe('api');
    });

    it('rejects invalid cron expressions', async () => {
      await expect(
        service.upsertManifestSchedule({
          agent_instance_id: '11111111-1111-4111-a111-111111111111',
          agent_name: 'sera',
          name: 'bad',
          type: 'cron',
          expression: 'invalid',
          task: 'nope',
          status: 'active',
        })
      ).rejects.toThrow('Invalid cron expression');
    });
  });

  describe('removeStaleManifestSchedules', () => {
    it('removes manifest schedules not in current list', async () => {
      const stale = [{ id: '77777777-7777-4777-a777-777777777777', name: 'old-schedule' }];
      mockQuery
        .mockResolvedValueOnce({ rows: stale }) // SELECT stale
        .mockResolvedValueOnce({ rows: [] }); // DELETE
      // AuditService.record is already mocked

      await service.removeStaleManifestSchedules('11111111-1111-4111-a111-111111111111', [
        'Reflection',
        'Goal Review',
      ]);

      // Verify DELETE was called
      const deleteCall = mockQuery.mock.calls.find(
        (call) => typeof call[0] === 'string' && call[0].includes('DELETE')
      );
      expect(deleteCall).toBeDefined();
      expect(deleteCall![1]).toEqual(['77777777-7777-4777-a777-777777777777']);
    });

    it('does nothing when no stale schedules exist', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] });

      await service.removeStaleManifestSchedules('11111111-1111-4111-a111-111111111111', [
        'Reflection',
      ]);

      // Only the SELECT query, no DELETE
      expect(mockQuery).toHaveBeenCalledTimes(1);
    });
  });
});
