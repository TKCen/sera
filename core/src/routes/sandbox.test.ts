import { describe, it, expect, vi, beforeEach } from 'vitest';
import express, { type Request, type Response, type NextFunction } from 'express';
import { createSandboxRouter } from './sandbox.js';
import { SandboxManager } from '../sandbox/SandboxManager.js';
import { PolicyViolationError } from '../sandbox/TierPolicy.js';
import type { AgentManifest } from '../agents/manifest/types.js';

// Define mocks as mock classes to prevent Vitest constructor type errors as required by memory guidelines
vi.mock('../sandbox/SandboxManager.js', () => {
  return {
    SandboxManager: class {
      spawn = vi.fn();
      exec = vi.fn();
      remove = vi.fn();
      getLogs = vi.fn();
      listContainers = vi.fn();
    },
  };
});

vi.mock('../sandbox/ToolRunner.js', () => {
  return {
    ToolRunner: class {
      runTool = vi.fn();
    },
  };
});

vi.mock('../agents/SubagentRunner.js', () => {
  return {
    SubagentRunner: class {
      spawnSubagent = vi.fn();
    },
  };
});

describe('Sandbox Routes', () => {
  let mockSandboxManager: any;
  let mockResolveManifest: any;
  let app: any;

  beforeEach(() => {
    vi.clearAllMocks();
    mockSandboxManager = new SandboxManager({} as any, {} as any);
    mockResolveManifest = vi.fn();

    const router = createSandboxRouter(mockSandboxManager, mockResolveManifest);

    app = express();
    app.use(express.json());
    app.use('/sandbox', router);

    // Basic error handler to avoid dumping errors to console during tests
    app.use((err: Error, req: Request, res: Response, next: NextFunction) => {
      res.status(500).json({ error: err.message });
    });
  });

  const validManifest = {
    metadata: {
      name: 'test-agent',
      tier: 2,
    },
    tools: {
      allowed: ['echo', 'ls'],
      denied: ['rm'],
    },
  } as unknown as AgentManifest;

  const invokeRoute = async (
    method: 'get' | 'post' | 'delete',
    path: string,
    reqData: any
  ) => {
    const router = createSandboxRouter(mockSandboxManager, mockResolveManifest);
    const req = {
      method: method.toUpperCase(),
      path,
      ...reqData,
    } as any;

    let resData: any;
    let statusCode: number = 200;

    const res = {
      status: vi.fn((code: number) => { statusCode = code; return res; }),
      json: vi.fn((data: any) => { resData = data; return res; }),
      send: vi.fn((data: any) => { resData = data; return res; }),
    } as any;

    let nextCalled = false;
    let nextErr: any;
    const next = vi.fn((err?: any) => {
      nextCalled = true;
      nextErr = err;
    });

    const route = router.stack.find((layer: any) =>
      layer.route && layer.route.path === path && layer.route.methods[method]
    );

    if (!route) {
      throw new Error(`Route ${method.toUpperCase()} ${path} not found`);
    }

    const handler = route.route.stack[0].handle;
    await handler(req, res, next);

    return { statusCode, resData, nextCalled, nextErr };
  };

  describe('POST /spawn', () => {
    it('should return 400 if agentName is missing', async () => {
      const { statusCode, resData } = await invokeRoute('post', '/spawn', { body: {} });
      expect(statusCode).toBe(400);
      expect(resData.error).toBe('agentName is required');
    });

    it('should return 404 if manifest not found', async () => {
      mockResolveManifest.mockResolvedValue(undefined);
      const { statusCode, resData } = await invokeRoute('post', '/spawn', { body: { agentName: 'missing' } });
      expect(statusCode).toBe(404);
      expect(resData.error).toBe('Agent "missing" not found');
    });

    it('should return 400 if type or image is missing', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      const { statusCode, resData } = await invokeRoute('post', '/spawn', { body: { agentName: 'test-agent' } });
      expect(statusCode).toBe(400);
      expect(resData.error).toBe('type and image are required');
    });

    it('should return 403 if tier is invalid or missing', async () => {
      const noTierManifest = { metadata: { name: 'no-tier' } } as any;
      mockResolveManifest.mockResolvedValue(noTierManifest);
      const { statusCode, resData } = await invokeRoute('post', '/spawn', {
        body: { agentName: 'no-tier', type: 'test', image: 'test:latest' }
      });
      expect(statusCode).toBe(403);
      expect(resData.error).toBe('Agent manifest must define a valid security tier');
    });

    it('should successfully spawn and return 201', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      mockSandboxManager.spawn.mockResolvedValue({ containerId: '123' });

      const { statusCode, resData } = await invokeRoute('post', '/spawn', {
        body: { agentName: 'test-agent', type: 'test', image: 'test:latest', command: ['sh'] }
      });

      expect(statusCode).toBe(201);
      expect(resData).toEqual({ containerId: '123' });
      expect(mockSandboxManager.spawn).toHaveBeenCalledWith(validManifest, expect.objectContaining({
        type: 'test', image: 'test:latest', command: ['sh']
      }));
    });

    it('should handle PolicyViolationError', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      mockSandboxManager.spawn.mockRejectedValue(new PolicyViolationError('Violated', 'test-agent', 'code'));

      const { statusCode, resData } = await invokeRoute('post', '/spawn', {
        body: { agentName: 'test-agent', type: 'test', image: 'test:latest' }
      });

      expect(statusCode).toBe(403);
      expect(resData.error).toBe('Violated');
      expect(resData.violation).toBe('code');
    });
  });

  describe('POST /exec', () => {
    it('should return 400 if containerId or command are missing', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      const { statusCode, resData } = await invokeRoute('post', '/exec', {
        body: { agentName: 'test-agent', containerId: '123' } // missing command
      });
      expect(statusCode).toBe(400);
    });

    it('should return 403 if tool is in denied list', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      const { statusCode, resData } = await invokeRoute('post', '/exec', {
        body: { agentName: 'test-agent', containerId: '123', command: ['rm', '-rf', '/'] }
      });
      expect(statusCode).toBe(403);
      expect(resData.violation).toBe('tool_denied');
    });

    it('should return 403 if tool is not in allowed list', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      const { statusCode, resData } = await invokeRoute('post', '/exec', {
        body: { agentName: 'test-agent', containerId: '123', command: ['wget', 'http://'] }
      });
      expect(statusCode).toBe(403);
      expect(resData.violation).toBe('tool_not_allowed');
    });

    it('should successfully exec', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      mockSandboxManager.exec.mockResolvedValue({ stdout: 'hi' });
      const { statusCode, resData } = await invokeRoute('post', '/exec', {
        body: { agentName: 'test-agent', containerId: '123', command: ['echo', 'hi'] }
      });
      expect(statusCode).toBe(200);
      expect(resData).toEqual({ stdout: 'hi' });
    });
  });

  describe('DELETE /:id', () => {
    it('should remove container successfully', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      mockSandboxManager.remove.mockResolvedValue();
      const { statusCode, resData } = await invokeRoute('delete', '/:id', {
        params: { id: '123' },
        query: { agentName: 'test-agent' }
      });
      expect(statusCode).toBe(200);
      expect(resData).toEqual({ success: true });
      expect(mockSandboxManager.remove).toHaveBeenCalledWith(validManifest, '123');
    });
  });

  describe('GET /:id/logs', () => {
    it('should return logs', async () => {
      mockSandboxManager.getLogs.mockResolvedValue('logs content');
      const { statusCode, resData } = await invokeRoute('get', '/:id/logs', {
        params: { id: '123' },
        query: { tail: '10' }
      });
      expect(statusCode).toBe(200);
      expect(resData).toEqual({ logs: 'logs content' });
      expect(mockSandboxManager.getLogs).toHaveBeenCalledWith('123', 10);
    });
  });

  describe('GET /', () => {
    it('should list containers', async () => {
      mockSandboxManager.listContainers.mockReturnValue([{ id: '123' }]);
      const { statusCode, resData } = await invokeRoute('get', '/', {
        query: { agentName: 'test-agent' }
      });
      expect(statusCode).toBe(200);
      expect(resData).toEqual([{ id: '123' }]);
      expect(mockSandboxManager.listContainers).toHaveBeenCalledWith('test-agent');
    });
  });

  describe('POST /tool', () => {
    it('should return 400 if toolName or command are missing', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      const { statusCode, resData } = await invokeRoute('post', '/tool', {
        body: { agentName: 'test-agent' }
      });
      expect(statusCode).toBe(400);
    });
  });

  describe('POST /subagent', () => {
    it('should return 400 if subagentRole or task are missing', async () => {
      mockResolveManifest.mockResolvedValue(validManifest);
      const { statusCode, resData } = await invokeRoute('post', '/subagent', {
        body: { agentName: 'test-agent' }
      });
      expect(statusCode).toBe(400);
    });
  });
});
