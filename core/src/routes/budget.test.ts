import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createBudgetRouter } from './budget.js';
import { query } from '../lib/database.js';
import type { Request, Response } from 'express';

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

vi.mock('../lib/logger.js', () => {
  return {
    Logger: class {
      info = vi.fn();
      error = vi.fn();
      warn = vi.fn();
    },
  };
});

vi.mock('express', () => {
  const expressRouterMock = {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
    use: vi.fn(),
  };

  const RouterMock = vi.fn(() => expressRouterMock);

  return {
    Router: RouterMock,
    default: {
      Router: RouterMock,
      json: vi.fn(),
    },
  };
});

import { Router } from 'express';

describe('Budget Route', () => {
  let mockMeteringService: any;
  let req: Partial<Request>;
  let res: Partial<Response>;
  let jsonMock: any;
  let statusMock: any;
  let routerMock: any;

  beforeEach(() => {
    vi.clearAllMocks();
    mockMeteringService = {
      checkBudget: vi.fn(),
    };

    // Create a mock for the express router instance returned by Router()
    routerMock = {
      get: vi.fn(),
      post: vi.fn(),
      put: vi.fn(),
      delete: vi.fn(),
      use: vi.fn(),
    };

    // Override the express Router mock to return our specific instance for this test
    vi.mocked(Router).mockReturnValue(routerMock);

    jsonMock = vi.fn();
    statusMock = vi.fn().mockReturnValue({ json: jsonMock });
    res = {
      json: jsonMock,
      status: statusMock,
    };
    req = {
      params: {},
    };
  });

  describe('GET /api/budget', () => {
    it('should return global budget totals', async () => {
      createBudgetRouter(mockMeteringService);
      const handler = routerMock.get.mock.calls.find((call: any[]) => call[0] === '/')[1];

      const mockRows = [
        { date: new Date('2023-10-01T00:00:00Z'), total_tokens: '1000' },
        { date: new Date('2023-10-02T00:00:00Z'), total_tokens: '1500' },
      ];
      vi.mocked(query).mockResolvedValueOnce({
        rows: mockRows,
        rowCount: 2,
        command: 'SELECT',
        oid: 0,
        fields: [],
      } as any);

      await handler(req as Request, res as Response);

      expect(jsonMock).toHaveBeenCalledWith({
        usage: [
          { date: '2023-10-01', totalTokens: 1000 },
          { date: '2023-10-02', totalTokens: 1500 },
        ],
      });
      expect(query).toHaveBeenCalledTimes(1);
    });

    it('should handle database errors gracefully', async () => {
      createBudgetRouter(mockMeteringService);
      const handler = routerMock.get.mock.calls.find((call: any[]) => call[0] === '/')[1];

      vi.mocked(query).mockRejectedValueOnce(new Error('DB Error'));

      await handler(req as Request, res as Response);

      expect(statusMock).toHaveBeenCalledWith(500);
      expect(jsonMock).toHaveBeenCalledWith({ error: 'Internal server error' });
    });
  });

  describe('GET /api/budget/agents', () => {
    it('should return per-agent budget rankings', async () => {
      createBudgetRouter(mockMeteringService);
      const handler = routerMock.get.mock.calls.find((call: any[]) => call[0] === '/agents')[1];

      const mockRows = [
        { agent_id: 'agent-1', total_tokens: '5000' },
        { agent_id: 'agent-2', total_tokens: '3000' },
      ];
      vi.mocked(query).mockResolvedValueOnce({
        rows: mockRows,
        rowCount: 2,
        command: 'SELECT',
        oid: 0,
        fields: [],
      } as any);

      await handler(req as Request, res as Response);

      expect(jsonMock).toHaveBeenCalledWith({
        rankings: [
          { agentId: 'agent-1', totalTokens: 5000 },
          { agentId: 'agent-2', totalTokens: 3000 },
        ],
      });
      expect(query).toHaveBeenCalledTimes(1);
    });
  });

  describe('GET /api/budget/agents/:id', () => {
    it('should return single agent usage history', async () => {
      createBudgetRouter(mockMeteringService);
      const handler = routerMock.get.mock.calls.find((call: any[]) => call[0] === '/agents/:id')[1];

      req.params = { id: 'agent-1' };
      const mockRows = [
        { date: new Date('2023-10-01T00:00:00Z'), total_tokens: '200' },
        { date: new Date('2023-10-02T00:00:00Z'), total_tokens: '300' },
      ];
      vi.mocked(query).mockResolvedValueOnce({
        rows: mockRows,
        rowCount: 2,
        command: 'SELECT',
        oid: 0,
        fields: [],
      } as any);

      await handler(req as Request, res as Response);

      expect(jsonMock).toHaveBeenCalledWith({
        agentId: 'agent-1',
        usage: [
          { date: '2023-10-01', totalTokens: 200 },
          { date: '2023-10-02', totalTokens: 300 },
        ],
      });
      expect(query).toHaveBeenCalledWith(expect.any(String), ['agent-1']);
    });
  });

  describe('GET /api/budget/agents/:id/budget', () => {
    it('should return 503 if MeteringService is not available', async () => {
      createBudgetRouter();
      const handler = routerMock.get.mock.calls.find(
        (call: any[]) => call[0] === '/agents/:id/budget'
      )[1];

      req.params = { id: 'agent-1' };
      await handler(req as Request, res as Response);

      expect(statusMock).toHaveBeenCalledWith(503);
      expect(jsonMock).toHaveBeenCalledWith({ error: 'MeteringService not available' });
    });

    it('should return agent budget status', async () => {
      createBudgetRouter(mockMeteringService);
      const handler = routerMock.get.mock.calls.find(
        (call: any[]) => call[0] === '/agents/:id/budget'
      )[1];

      req.params = { id: 'agent-1' };
      const mockStatus = { remaining: 1000, limit: 2000 };
      mockMeteringService.checkBudget.mockResolvedValueOnce(mockStatus);

      await handler(req as Request, res as Response);

      expect(jsonMock).toHaveBeenCalledWith({ agentId: 'agent-1', ...mockStatus });
      expect(mockMeteringService.checkBudget).toHaveBeenCalledWith('agent-1');
    });

    it('should handle errors gracefully', async () => {
      createBudgetRouter(mockMeteringService);
      const handler = routerMock.get.mock.calls.find(
        (call: any[]) => call[0] === '/agents/:id/budget'
      )[1];

      req.params = { id: 'agent-1' };
      mockMeteringService.checkBudget.mockRejectedValueOnce(new Error('Service Error'));

      await handler(req as Request, res as Response);

      expect(statusMock).toHaveBeenCalledWith(500);
      expect(jsonMock).toHaveBeenCalledWith({ error: 'Internal server error' });
    });
  });
});
