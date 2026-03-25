import { describe, it, expect, vi, beforeEach } from 'vitest';
import express, { type Request, type Response, type NextFunction } from 'express';
import { createCirclesDbRouter } from './circles-db.js';
import { CircleService } from '../circles/CircleService.js';

vi.mock('../circles/CircleService.js', () => ({
  CircleService: {
    getInstance: vi.fn(),
  },
}));

vi.mock('../lib/logger.js', () => ({
  Logger: class {
    error = vi.fn();
    info = vi.fn();
    warn = vi.fn();
  },
}));

describe('Circles DB Router', () => {
  let app: any;
  let mockCircleService: any;
  let mockOrchestrator: any;

  beforeEach(() => {
    vi.clearAllMocks();

    mockCircleService = {
      listCircles: vi.fn().mockResolvedValue([{ id: 'c1', name: 'circle-1' }]),
      createCircle: vi.fn(),
      getCircle: vi.fn(),
      getMembers: vi.fn().mockResolvedValue([]),
      updateCircle: vi.fn(),
      deleteCircle: vi.fn(),
      createPartySession: vi.fn(),
      getPartySession: vi.fn(),
      appendPartyRound: vi.fn(),
      completePartySession: vi.fn(),
    };

    (CircleService.getInstance as any).mockReturnValue(mockCircleService);

    mockOrchestrator = {
      getIntercom: vi.fn(),
      getAgent: vi.fn(),
    };

    const router = createCirclesDbRouter(mockOrchestrator);

    app = express();
    app.use(express.json());
    app.use('/circles-db', router);

    app.use((err: Error, req: Request, res: Response, next: NextFunction) => {
      res.status(500).json({ error: err.message });
    });
  });

  const invokeRoute = async (
    method: 'get' | 'post' | 'put' | 'delete',
    pathStr: string,
    reqData: any = {}
  ) => {
    const router = createCirclesDbRouter(mockOrchestrator);
    const req = { method: method.toUpperCase(), path: pathStr, ...reqData } as any;

    let resData: any;
    let statusCode: number = 200;

    const res = {
      status: vi.fn((code: number) => {
        statusCode = code;
        return res;
      }),
      json: vi.fn((data: any) => {
        resData = data;
        return res;
      }),
      send: vi.fn((data: any) => {
        resData = data;
        return res;
      }),
    } as any;

    let nextCalled = false;
    let nextErr: any;
    const next = vi.fn((err?: any) => {
      nextCalled = true;
      nextErr = err;
    });

    const route = router.stack.find(
      (layer: any) => layer.route && layer.route.path === pathStr && layer.route.methods[method]
    );

    if (!route) {
      throw new Error(`Route ${method.toUpperCase()} ${pathStr} not found`);
    }

    const handler = route.route.stack[0].handle;
    await handler(req, res, next);

    return { statusCode, resData, nextCalled, nextErr };
  };

  describe('GET /', () => {
    it('should list all circles', async () => {
      const { statusCode, resData } = await invokeRoute('get', '/');
      expect(statusCode).toBe(200);
      expect(resData).toEqual([{ id: 'c1', name: 'circle-1' }]);
    });
  });

  describe('POST /', () => {
    it('should create a new circle', async () => {
      mockCircleService.createCircle.mockResolvedValue({ id: 'c2', name: 'new-circle' });
      const { statusCode, resData } = await invokeRoute('post', '/', {
        body: { name: 'new-circle', displayName: 'New Circle' },
      });
      expect(statusCode).toBe(201);
      expect(resData.id).toBe('c2');
    });

    it('should return 400 if name is missing', async () => {
      const { statusCode, resData } = await invokeRoute('post', '/', {
        body: { displayName: 'New Circle' },
      });
      expect(statusCode).toBe(400);
      expect(resData.error).toBe('name and displayName are required');
    });

    it('should handle unique constraint violations', async () => {
      const err = new Error('duplicate key value violates unique constraint');
      (err as any).code = '23505';
      mockCircleService.createCircle.mockRejectedValue(err);

      const { statusCode, resData } = await invokeRoute('post', '/', {
        body: { name: 'existing', displayName: 'Existing' },
      });
      expect(statusCode).toBe(409);
      expect(resData.error).toContain('already exists');
    });
  });

  describe('GET /:id', () => {
    it('should return circle by id', async () => {
      mockCircleService.getCircle.mockResolvedValue({ id: 'c1', name: 'circle-1' });
      mockCircleService.getMembers.mockResolvedValue([{ agentId: 'a1' }]);

      const { statusCode, resData } = await invokeRoute('get', '/:id', { params: { id: 'c1' } });
      expect(statusCode).toBe(200);
      expect(resData.name).toBe('circle-1');
      expect(resData.members).toEqual([{ agentId: 'a1' }]);
    });

    it('should return 404 if circle not found', async () => {
      mockCircleService.getCircle.mockResolvedValue(undefined);
      const { statusCode, resData } = await invokeRoute('get', '/:id', {
        params: { id: 'missing' },
      });
      expect(statusCode).toBe(404);
    });
  });

  describe('PUT /:id', () => {
    it('should update circle', async () => {
      mockCircleService.updateCircle.mockResolvedValue({ id: 'c1', displayName: 'Updated' });
      const { statusCode, resData } = await invokeRoute('put', '/:id', {
        params: { id: 'c1' },
        body: { displayName: 'Updated' },
      });
      expect(statusCode).toBe(200);
      expect(resData.displayName).toBe('Updated');
    });
  });

  describe('DELETE /:id', () => {
    it('should delete circle', async () => {
      mockCircleService.deleteCircle.mockResolvedValue(undefined);
      const { statusCode, resData } = await invokeRoute('delete', '/:id', { params: { id: 'c1' } });
      expect(statusCode).toBe(200);
      expect(resData.success).toBe(true);
    });

    it('should return 409 if circle has active agents', async () => {
      mockCircleService.deleteCircle.mockRejectedValue(
        new Error('Cannot delete circle with active agents')
      );
      const { statusCode, resData } = await invokeRoute('delete', '/:id', { params: { id: 'c1' } });
      expect(statusCode).toBe(409);
      expect(resData.error).toContain('active agent');
    });
  });

  // Additional tests for /:id/members and /:id/party can go here
});
