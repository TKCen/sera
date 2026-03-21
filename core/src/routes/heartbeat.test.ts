import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createHeartbeatRouter } from './heartbeat.js';

// We mock the middleware dynamically to allow per-test behavior
vi.mock('../auth/authMiddleware.js', () => ({
  createAuthMiddleware: () => (req: any, res: any, next: any) => {
    if (!req.agentIdentity) {
      req.agentIdentity = { agentId: req.params.id };
    }
    next();
  },
}));

describe('Heartbeat Routes', () => {
  let app!: express.Express;
  let orchestratorMock!: any;
  let identityServiceMock!: any;
  let authServiceMock!: any;

  beforeEach(() => {
    orchestratorMock = {
      registerHeartbeat: vi.fn(),
      getUnhealthyInstances: vi.fn(),
    };
    identityServiceMock = {};
    authServiceMock = {};

    app = express();
    app.use(express.json());
    // Apply custom middleware to override agentIdentity when needed
    app.use('/api/agents/:id/heartbeat', (req: any, res: any, next: any) => {
      if (req.headers['x-mock-identity']) {
        req.agentIdentity = { agentId: req.headers['x-mock-identity'] };
      }
      next();
    });

    app.use(
      '/api/agents',
      createHeartbeatRouter(orchestratorMock, identityServiceMock, authServiceMock)
    );
  });

  describe('POST /:id/heartbeat', () => {
    it('registers heartbeat when identity matches url param', async () => {
      const res = await request(app).post('/api/agents/test-agent-1/heartbeat');

      expect(res.status).toBe(200);
      expect(res.body.status).toBe('ok');
      expect(res.body.timestamp).toBeDefined();
      expect(orchestratorMock.registerHeartbeat).toHaveBeenCalledWith('test-agent-1');
    });

    it('returns 403 when identity does not match url param', async () => {
      const res = await request(app)
        .post('/api/agents/test-agent-1/heartbeat')
        .set('x-mock-identity', 'wrong-agent-id'); // Override identity via header

      expect(res.status).toBe(403);
      expect(res.body.error).toBe('Token agentId does not match URL');
      expect(orchestratorMock.registerHeartbeat).not.toHaveBeenCalled();
    });
  });

  describe('GET /health', () => {
    it('returns list of unhealthy instances', async () => {
      const mockDate = new Date();
      orchestratorMock.getUnhealthyInstances.mockReturnValue([
        { instanceId: 'unhealthy-agent-1', lastSeen: mockDate }
      ]);

      const res = await request(app).get('/api/agents/health');

      expect(res.status).toBe(200);
      expect(res.body.unhealthy).toHaveLength(1);
      expect(res.body.unhealthy[0]).toEqual({
        instanceId: 'unhealthy-agent-1',
        lastSeen: mockDate.toISOString()
      });
    });

    it('returns empty list when all agents are healthy', async () => {
      orchestratorMock.getUnhealthyInstances.mockReturnValue([]);

      const res = await request(app).get('/api/agents/health');

      expect(res.status).toBe(200);
      expect(res.body.unhealthy).toEqual([]);
    });
  });
});
