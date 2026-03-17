import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock the database module before importing MeteringService
vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

import { MeteringService } from './MeteringService.js';
import { query } from '../lib/database.js';

const mockQuery = vi.mocked(query);

describe('MeteringService', () => {
  let service: MeteringService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = new MeteringService();
  });

  describe('recordUsage', () => {
    it('should insert a token_usage row', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 1, command: 'INSERT', oid: 0, fields: [] });

      await service.recordUsage({
        agentId: 'agent-001',
        circleId: 'dev-circle',
        model: 'gpt-4',
        promptTokens: 100,
        completionTokens: 50,
        totalTokens: 150,
      });

      expect(mockQuery).toHaveBeenCalledOnce();
      expect(mockQuery.mock.calls[0]![0]).toContain('INSERT INTO token_usage');
      expect(mockQuery.mock.calls[0]![1]).toEqual([
        'agent-001', 'dev-circle', 'gpt-4', 100, 50, 150,
      ]);
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
    it('should allow when usage is under quota', async () => {
      // 1st call: quota lookup
      mockQuery.mockResolvedValueOnce({
        rows: [{ max_tokens_per_hour: 10000, max_tokens_per_day: 100000 }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });
      // 2nd call: hourly usage
      mockQuery.mockResolvedValueOnce({
        rows: [{ total: '500' }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });
      // 3rd call: daily usage
      mockQuery.mockResolvedValueOnce({
        rows: [{ total: '5000' }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });

      const status = await service.checkBudget('agent-001');
      expect(status.allowed).toBe(true);
      expect(status.hourlyUsed).toBe(500);
      expect(status.hourlyQuota).toBe(10000);
      expect(status.dailyUsed).toBe(5000);
      expect(status.dailyQuota).toBe(100000);
    });

    it('should deny when hourly usage exceeds quota', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ max_tokens_per_hour: 1000, max_tokens_per_day: 100000 }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });
      mockQuery.mockResolvedValueOnce({
        rows: [{ total: '1500' }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });
      mockQuery.mockResolvedValueOnce({
        rows: [{ total: '1500' }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });

      const status = await service.checkBudget('agent-001');
      expect(status.allowed).toBe(false);
    });

    it('should use default quotas when no quota row exists', async () => {
      // No quota row
      mockQuery.mockResolvedValueOnce({
        rows: [],
        rowCount: 0,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });
      // Hourly usage
      mockQuery.mockResolvedValueOnce({
        rows: [{ total: '100' }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });
      // Daily usage
      mockQuery.mockResolvedValueOnce({
        rows: [{ total: '200' }],
        rowCount: 1,
        command: 'SELECT',
        oid: 0,
        fields: [],
      });

      const status = await service.checkBudget('agent-new');
      expect(status.allowed).toBe(true);
      // Should use the default quota (100000 hourly)
      expect(status.hourlyQuota).toBe(100000);
    });
  });
});
