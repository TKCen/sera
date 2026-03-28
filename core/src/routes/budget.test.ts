import { describe, it, expect, vi, beforeEach } from 'vitest';
import express from 'express';
import request from 'supertest';
import { createBudgetRouter } from './budget.js';
import { query } from '../lib/database.js';
import type { MeteringService } from '../metering/MeteringService.js';

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

vi.mock('../lib/logger.js', () => ({
  Logger: class {
    error = vi.fn();
    info = vi.fn();
    warn = vi.fn();
  },
}));

describe('BudgetRouter', () => {
  let app: express.Express;
  let mockMeteringService: Partial<MeteringService>;

  beforeEach(() => {
    vi.clearAllMocks();

    mockMeteringService = {
      checkBudget: vi.fn(),
    };

    app = express();
    app.use(express.json());
    app.use('/api/budget', createBudgetRouter(mockMeteringService as MeteringService));
  });

  describe('GET /api/budget', () => {
    it('returns global totals grouped by day', async () => {
      vi.mocked(query).mockResolvedValueOnce({
        rows: [
          { date: new Date('2023-10-01T00:00:00Z'), total_tokens: '1000' },
          { date: new Date('2023-10-02T00:00:00Z'), total_tokens: '2500' },
        ],
        command: 'SELECT',
        rowCount: 2,
        oid: 0,
        fields: [],
      });

      const res = await request(app).get('/api/budget');

      expect(res.status).toBe(200);
      expect(res.body).toEqual({
        usage: [
          { date: '2023-10-01', totalTokens: 1000 },
          { date: '2023-10-02', totalTokens: 2500 },
        ],
      });
      expect(query).toHaveBeenCalledTimes(1);
    });

    it('returns 500 on database error', async () => {
      vi.mocked(query).mockRejectedValueOnce(new Error('DB Error'));

      const res = await request(app).get('/api/budget');

      expect(res.status).toBe(500);
      expect(res.body).toEqual({ error: 'Internal server error' });
    });
  });

  describe('GET /api/budget/agents', () => {
    it('returns agent rankings', async () => {
      vi.mocked(query).mockResolvedValueOnce({
        rows: [
          { agent_id: 'agent1', total_tokens: '5000' },
          { agent_id: 'agent2', total_tokens: '3000' },
        ],
        command: 'SELECT',
        rowCount: 2,
        oid: 0,
        fields: [],
      });

      const res = await request(app).get('/api/budget/agents');

      expect(res.status).toBe(200);
      expect(res.body).toEqual({
        rankings: [
          { agentId: 'agent1', totalTokens: 5000 },
          { agentId: 'agent2', totalTokens: 3000 },
        ],
      });
    });

    it('returns 500 on database error', async () => {
      vi.mocked(query).mockRejectedValueOnce(new Error('DB Error'));

      const res = await request(app).get('/api/budget/agents');

      expect(res.status).toBe(500);
      expect(res.body).toEqual({ error: 'Internal server error' });
    });
  });

  describe('GET /api/budget/agents/:id', () => {
    it('returns single agent usage history', async () => {
      vi.mocked(query).mockResolvedValueOnce({
        rows: [
          { date: new Date('2023-10-01T00:00:00Z'), total_tokens: '500' },
        ],
        command: 'SELECT',
        rowCount: 1,
        oid: 0,
        fields: [],
      });

      const res = await request(app).get('/api/budget/agents/agent1');

      expect(res.status).toBe(200);
      expect(res.body).toEqual({
        agentId: 'agent1',
        usage: [
          { date: '2023-10-01', totalTokens: 500 },
        ],
      });
      expect(query).toHaveBeenCalledWith(expect.any(String), ['agent1']);
    });

    it('returns 500 on database error', async () => {
      vi.mocked(query).mockRejectedValueOnce(new Error('DB Error'));

      const res = await request(app).get('/api/budget/agents/agent1');

      expect(res.status).toBe(500);
      expect(res.body).toEqual({ error: 'Internal server error' });
    });
  });

  describe('GET /api/budget/agents/:id/budget', () => {
    it('returns budget status for a specific agent', async () => {
      vi.mocked(mockMeteringService.checkBudget!).mockResolvedValueOnce({
        allowed: true,
        hourlyUsed: 100,
        hourlyQuota: 1000,
        dailyUsed: 500,
        dailyQuota: 5000,
      });

      const res = await request(app).get('/api/budget/agents/agent1/budget');

      expect(res.status).toBe(200);
      expect(res.body).toEqual({
        agentId: 'agent1',
        allowed: true,
        hourlyUsed: 100,
        hourlyQuota: 1000,
        dailyUsed: 500,
        dailyQuota: 5000,
      });
      expect(mockMeteringService.checkBudget).toHaveBeenCalledWith('agent1');
    });

    it('returns 503 if MeteringService is not available', async () => {
      const appNoService = express();
      appNoService.use(express.json());
      appNoService.use('/api/budget', createBudgetRouter());

      const res = await request(appNoService).get('/api/budget/agents/agent1/budget');

      expect(res.status).toBe(503);
      expect(res.body).toEqual({ error: 'MeteringService not available' });
    });

    it('returns 500 on MeteringService error', async () => {
      vi.mocked(mockMeteringService.checkBudget!).mockRejectedValueOnce(new Error('Service Error'));

      const res = await request(app).get('/api/budget/agents/agent1/budget');

      expect(res.status).toBe(500);
      expect(res.body).toEqual({ error: 'Internal server error' });
    });
  });
});
