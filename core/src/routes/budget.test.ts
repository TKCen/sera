import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createBudgetRouter } from './budget.js';

// Mock the database query wrapper
const queryMock = vi.fn();
vi.mock('../lib/database.js', () => ({
  query: (...args: any[]) => queryMock(...args),
}));

describe('Budget Routes', () => {
  let app!: express.Express;
  let meteringServiceMock!: any;

  beforeEach(() => {
    vi.resetAllMocks();
    meteringServiceMock = {
      checkBudget: vi.fn(),
    };

    app = express();
    app.use(express.json());
    app.use('/api/budget', createBudgetRouter(meteringServiceMock));
  });

  describe('GET /', () => {
    it('returns global budget usage', async () => {
      const mockDate = new Date('2023-10-01T00:00:00Z');
      queryMock.mockResolvedValueOnce({
        rows: [{ date: mockDate, total_tokens: '1500' }],
      });

      const res = await request(app).get('/api/budget');

      expect(res.status).toBe(200);
      expect(res.body.usage).toHaveLength(1);
      expect(res.body.usage[0]).toEqual({
        date: '2023-10-01',
        totalTokens: 1500,
      });
      expect(queryMock).toHaveBeenCalledTimes(1);
    });

    it('handles database errors gracefully', async () => {
      queryMock.mockRejectedValueOnce(new Error('DB Error'));

      const res = await request(app).get('/api/budget');

      expect(res.status).toBe(500);
      expect(res.body.error).toBe('Internal server error');
    });
  });

  describe('GET /agents', () => {
    it('returns agent rankings', async () => {
      queryMock.mockResolvedValueOnce({
        rows: [
          { agent_id: 'agent-1', total_tokens: '5000' },
          { agent_id: 'agent-2', total_tokens: '3000' },
        ],
      });

      const res = await request(app).get('/api/budget/agents');

      expect(res.status).toBe(200);
      expect(res.body.rankings).toHaveLength(2);
      expect(res.body.rankings[0]).toEqual({
        agentId: 'agent-1',
        totalTokens: 5000,
      });
      expect(queryMock).toHaveBeenCalledTimes(1);
    });

    it('handles database errors gracefully', async () => {
      queryMock.mockRejectedValueOnce(new Error('DB Error'));

      const res = await request(app).get('/api/budget/agents');

      expect(res.status).toBe(500);
      expect(res.body.error).toBe('Internal server error');
    });
  });

  describe('GET /agents/:id', () => {
    it('returns single agent usage history', async () => {
      const mockDate = new Date('2023-10-02T00:00:00Z');
      queryMock.mockResolvedValueOnce({
        rows: [{ date: mockDate, total_tokens: '800' }],
      });

      const res = await request(app).get('/api/budget/agents/agent-1');

      expect(res.status).toBe(200);
      expect(res.body.agentId).toBe('agent-1');
      expect(res.body.usage).toHaveLength(1);
      expect(res.body.usage[0]).toEqual({
        date: '2023-10-02',
        totalTokens: 800,
      });
      expect(queryMock).toHaveBeenCalledWith(expect.any(String), ['agent-1']);
    });

    it('handles database errors gracefully', async () => {
      queryMock.mockRejectedValueOnce(new Error('DB Error'));

      const res = await request(app).get('/api/budget/agents/agent-1');

      expect(res.status).toBe(500);
      expect(res.body.error).toBe('Internal server error');
    });
  });

  describe('GET /agents/:id/budget', () => {
    it('returns budget status from metering service', async () => {
      meteringServiceMock.checkBudget.mockResolvedValueOnce({
        hourlyTokens: 100,
        hourlyLimit: 1000,
        dailyTokens: 500,
        dailyLimit: 5000,
      });

      const res = await request(app).get('/api/budget/agents/agent-1/budget');

      expect(res.status).toBe(200);
      expect(res.body).toEqual({
        agentId: 'agent-1',
        hourlyTokens: 100,
        hourlyLimit: 1000,
        dailyTokens: 500,
        dailyLimit: 5000,
      });
      expect(meteringServiceMock.checkBudget).toHaveBeenCalledWith('agent-1');
    });

    it('returns 503 if metering service is not provided', async () => {
      const appNoService = express();
      appNoService.use(express.json());
      appNoService.use('/api/budget', createBudgetRouter(undefined));

      const res = await request(appNoService).get('/api/budget/agents/agent-1/budget');

      expect(res.status).toBe(503);
      expect(res.body.error).toBe('MeteringService not available');
    });

    it('handles metering service errors gracefully', async () => {
      meteringServiceMock.checkBudget.mockRejectedValueOnce(new Error('Metering Error'));

      const res = await request(app).get('/api/budget/agents/agent-1/budget');

      expect(res.status).toBe(500);
      expect(res.body.error).toBe('Internal server error');
    });
  });
});
