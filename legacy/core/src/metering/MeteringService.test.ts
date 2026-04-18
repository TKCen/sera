import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock AuditService to prevent it from using the pool
vi.mock('../audit/AuditService.js', () => ({
  AuditService: {
    getInstance: () => ({
      record: vi.fn().mockResolvedValue(undefined),
    }),
  },
}));

// Mock the database module before importing MeteringService
const mockClientQuery = vi.fn();
const mockClientRelease = vi.fn();
const mockClient = {
  query: mockClientQuery,
  release: mockClientRelease,
};

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
  pool: {
    connect: vi.fn(),
  },
}));

import { MeteringService } from './MeteringService.js';
import { query, pool } from '../lib/database.js';

const mockQuery = vi.mocked(query);
const mockPool = vi.mocked(pool);

describe('MeteringService', () => {
  let service: MeteringService;

  beforeEach(() => {
    vi.clearAllMocks();
    mockClientQuery.mockResolvedValue({
      rows: [],
      rowCount: 0,
      command: 'SELECT',
      oid: 0,
      fields: [],
    });
    mockClientRelease.mockReturnValue(undefined);
    (mockPool.connect as ReturnType<typeof vi.fn>).mockResolvedValue(mockClient);
    service = new MeteringService();
  });

  describe('recordUsage', () => {
    it('should insert into both token_usage and usage_events', async () => {
      // Both parallel queries succeed
      mockQuery
        .mockResolvedValueOnce({ rows: [], rowCount: 1, command: 'INSERT', oid: 0, fields: [] })
        .mockResolvedValueOnce({ rows: [], rowCount: 1, command: 'INSERT', oid: 0, fields: [] });

      await service.recordUsage({
        agentId: 'agent-001',
        circleId: 'dev-circle',
        model: 'gpt-4',
        promptTokens: 100,
        completionTokens: 50,
        totalTokens: 150,
        costUsd: 0.003,
        latencyMs: 250,
        status: 'success',
      });

      expect(mockQuery).toHaveBeenCalledTimes(2);

      // Check token_usage insert
      const tokenUsageCall = mockQuery.mock.calls.find(
        (c) => typeof c[0] === 'string' && c[0].includes('INSERT INTO token_usage')
      );
      expect(tokenUsageCall).toBeDefined();
      expect(tokenUsageCall![1]).toEqual(['agent-001', 'dev-circle', 'gpt-4', 100, 50, 150]);

      // Check usage_events insert
      const usageEventsCall = mockQuery.mock.calls.find(
        (c) => typeof c[0] === 'string' && c[0].includes('INSERT INTO usage_events')
      );
      expect(usageEventsCall).toBeDefined();
      expect(usageEventsCall![1]).toEqual([
        'agent-001',
        'gpt-4',
        100,
        50,
        150,
        0.003,
        250,
        'success',
      ]);
    });

    it('should default status to success when not provided', async () => {
      mockQuery
        .mockResolvedValueOnce({ rows: [], rowCount: 1, command: 'INSERT', oid: 0, fields: [] })
        .mockResolvedValueOnce({ rows: [], rowCount: 1, command: 'INSERT', oid: 0, fields: [] });

      await service.recordUsage({
        agentId: 'agent-001',
        circleId: null,
        model: 'gpt-4',
        promptTokens: 10,
        completionTokens: 5,
        totalTokens: 15,
      });

      const usageEventsCall = mockQuery.mock.calls.find(
        (c) => typeof c[0] === 'string' && c[0].includes('INSERT INTO usage_events')
      )!;
      // status (index 7) should be 'success'
      expect(usageEventsCall[1]![7]).toBe('success');
    });
  });

  describe('getUsage', () => {
    it('should return summed tokens within the window', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ total: '2500' }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });

      const total = await service.getUsage('agent-001', 1);
      expect(total).toBe(2500);
      expect(mockQuery.mock.calls[0]![1]).toEqual(['agent-001', 1]);
    });

    it('should return 0 when no usage exists', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ total: '0' }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });

      const total = await service.getUsage('agent-002', 24);
      expect(total).toBe(0);
    });
  });

  describe('checkBudget', () => {
    /**
     * Helper: set up mockClientQuery responses for a checkBudget call.
     * Sequence: BEGIN, pg_advisory_xact_lock, quota SELECT, hourly SELECT, daily SELECT, COMMIT
     */
    function setupBudgetMocks(quotaRows: object[], hourlyTotal: string, dailyTotal: string) {
      mockClientQuery
        // BEGIN
        .mockResolvedValueOnce({ rows: [], rowCount: 0, command: 'BEGIN', oid: 0, fields: [] })
        // pg_advisory_xact_lock
        .mockResolvedValueOnce({ rows: [], rowCount: 0, command: 'SELECT', oid: 0, fields: [] })
        // quota lookup
        .mockResolvedValueOnce({
          rows: quotaRows,
          rowCount: quotaRows.length,
          command: 'SELECT',
          oid: 0,
          fields: [],
        })
        // hourly usage
        .mockResolvedValueOnce({
          rows: [{ total: hourlyTotal }],
          rowCount: 1,
          command: 'SELECT',
          oid: 0,
          fields: [],
        })
        // daily usage
        .mockResolvedValueOnce({
          rows: [{ total: dailyTotal }],
          rowCount: 1,
          command: 'SELECT',
          oid: 0,
          fields: [],
        })
        // COMMIT
        .mockResolvedValueOnce({ rows: [], rowCount: 0, command: 'COMMIT', oid: 0, fields: [] });
    }

    it('should allow when usage is under quota', async () => {
      setupBudgetMocks([{ max_tokens_per_hour: 10000, max_tokens_per_day: 100000 }], '500', '5000');

      const status = await service.checkBudget('agent-001');
      expect(status.allowed).toBe(true);
      expect(status.hourlyUsed).toBe(500);
      expect(status.hourlyQuota).toBe(10000);
      expect(status.dailyUsed).toBe(5000);
      expect(status.dailyQuota).toBe(100000);
      expect(mockClientRelease).toHaveBeenCalledTimes(1);
    });

    it('should deny when hourly usage exceeds quota', async () => {
      setupBudgetMocks([{ max_tokens_per_hour: 1000, max_tokens_per_day: 100000 }], '1500', '1500');

      const status = await service.checkBudget('agent-001');
      expect(status.allowed).toBe(false);
      expect(mockClientRelease).toHaveBeenCalledTimes(1);
    });

    it('should use default quotas when no quota row exists', async () => {
      setupBudgetMocks([], '100', '200');

      const status = await service.checkBudget('agent-new');
      expect(status.allowed).toBe(true);
      // Should use the default quota (100000 hourly)
      expect(status.hourlyQuota).toBe(100000);
      expect(mockClientRelease).toHaveBeenCalledTimes(1);
    });

    it('should allow when hourly quota is 0 (unlimited) regardless of usage', async () => {
      setupBudgetMocks([{ max_tokens_per_hour: 0, max_tokens_per_day: 1000000 }], '999999', '100');

      const status = await service.checkBudget('agent-unlimited-hourly');
      expect(status.allowed).toBe(true);
      expect(status.hourlyQuota).toBe(0);
      expect(mockClientRelease).toHaveBeenCalledTimes(1);
    });

    it('should allow when daily quota is 0 (unlimited) regardless of usage', async () => {
      setupBudgetMocks([{ max_tokens_per_hour: 10000, max_tokens_per_day: 0 }], '100', '99999999');

      const status = await service.checkBudget('agent-unlimited-daily');
      expect(status.allowed).toBe(true);
      expect(status.dailyQuota).toBe(0);
      expect(mockClientRelease).toHaveBeenCalledTimes(1);
    });

    it('should allow when both quotas are 0 (fully unlimited)', async () => {
      setupBudgetMocks([{ max_tokens_per_hour: 0, max_tokens_per_day: 0 }], '999999', '999999');

      const status = await service.checkBudget('agent-fully-unlimited');
      expect(status.allowed).toBe(true);
      expect(mockClientRelease).toHaveBeenCalledTimes(1);
    });

    it('should release the client and rollback on error', async () => {
      mockClientQuery
        .mockResolvedValueOnce({ rows: [], rowCount: 0, command: 'BEGIN', oid: 0, fields: [] })
        .mockRejectedValueOnce(new Error('DB error'))
        // ROLLBACK
        .mockResolvedValueOnce({ rows: [], rowCount: 0, command: 'ROLLBACK', oid: 0, fields: [] });

      await expect(service.checkBudget('agent-err')).rejects.toThrow('DB error');
      expect(mockClientRelease).toHaveBeenCalledTimes(1);
    });
  });
});
