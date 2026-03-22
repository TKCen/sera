import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createMCPRouter } from './mcp.js';
import { Request, Response } from 'express';

vi.mock('express', () => {
  const expressRouterMock = {
    post: vi.fn(),
    delete: vi.fn(),
  };

  const RouterMock = vi.fn(() => expressRouterMock);

  return {
    Router: RouterMock,
    default: {
      Router: RouterMock,
    },
  };
});

import { Router } from 'express';

describe('MCP Route', () => {
  let mockMCPRegistry: any;
  let mockSkillRegistry: any;
  let req: Partial<Request>;
  let res: Partial<Response>;
  let jsonMock: any;
  let statusMock: any;
  let routerMock: any;

  beforeEach(() => {
    vi.clearAllMocks();
    mockMCPRegistry = {
      registerContainerServer: vi.fn(),
      unregisterClient: vi.fn(),
    };
    mockSkillRegistry = {};

    routerMock = {
      post: vi.fn(),
      delete: vi.fn(),
    };

    vi.mocked(Router).mockReturnValue(routerMock);

    jsonMock = vi.fn();
    statusMock = vi.fn().mockReturnValue({ json: jsonMock });
    res = {
      json: jsonMock,
      status: statusMock,
    };
    req = {
      body: {},
      params: {},
    };
  });

  describe('POST /api/mcp-servers', () => {
    it('should register a new containerized MCP server from manifest', async () => {
      createMCPRouter(mockMCPRegistry, mockSkillRegistry);
      const handler = routerMock.post.mock.calls.find((call: any[]) => call[0] === '/')[1];

      req.body = { metadata: { name: 'test-server' } };

      await handler(req as Request, res as Response);

      expect(mockMCPRegistry.registerContainerServer).toHaveBeenCalledWith(req.body);
      expect(jsonMock).toHaveBeenCalledWith({
        message: 'MCP server "test-server" registered successfully',
      });
    });

    it('should return 400 for invalid manifest (missing body)', async () => {
      createMCPRouter(mockMCPRegistry, mockSkillRegistry);
      const handler = routerMock.post.mock.calls.find((call: any[]) => call[0] === '/')[1];

      req.body = undefined;

      await handler(req as Request, res as Response);

      expect(statusMock).toHaveBeenCalledWith(400);
      expect(jsonMock).toHaveBeenCalledWith({ error: 'Invalid manifest: missing metadata.name' });
    });

    it('should return 400 for invalid manifest (missing metadata)', async () => {
      createMCPRouter(mockMCPRegistry, mockSkillRegistry);
      const handler = routerMock.post.mock.calls.find((call: any[]) => call[0] === '/')[1];

      req.body = { other: 'data' };

      await handler(req as Request, res as Response);

      expect(statusMock).toHaveBeenCalledWith(400);
      expect(jsonMock).toHaveBeenCalledWith({ error: 'Invalid manifest: missing metadata.name' });
    });

    it('should handle registration errors gracefully', async () => {
      createMCPRouter(mockMCPRegistry, mockSkillRegistry);
      const handler = routerMock.post.mock.calls.find((call: any[]) => call[0] === '/')[1];

      req.body = { metadata: { name: 'test-server' } };
      mockMCPRegistry.registerContainerServer.mockRejectedValueOnce(
        new Error('Registration failed')
      );

      await handler(req as Request, res as Response);

      expect(statusMock).toHaveBeenCalledWith(500);
      expect(jsonMock).toHaveBeenCalledWith({ error: 'Registration failed' });
    });
  });

  describe('DELETE /api/mcp-servers/:name', () => {
    it('should unregister an MCP server successfully', async () => {
      createMCPRouter(mockMCPRegistry, mockSkillRegistry);
      const handler = routerMock.delete.mock.calls.find((call: any[]) => call[0] === '/:name')[1];

      req.params = { name: 'test-server' };
      mockMCPRegistry.unregisterClient.mockResolvedValueOnce(true);

      await handler(req as Request, res as Response);

      expect(mockMCPRegistry.unregisterClient).toHaveBeenCalledWith('test-server');
      expect(jsonMock).toHaveBeenCalledWith({
        message: 'MCP server "test-server" unregistered successfully',
      });
    });

    it('should return 404 if MCP server is not found', async () => {
      createMCPRouter(mockMCPRegistry, mockSkillRegistry);
      const handler = routerMock.delete.mock.calls.find((call: any[]) => call[0] === '/:name')[1];

      req.params = { name: 'unknown-server' };
      mockMCPRegistry.unregisterClient.mockResolvedValueOnce(false);

      await handler(req as Request, res as Response);

      expect(statusMock).toHaveBeenCalledWith(404);
      expect(jsonMock).toHaveBeenCalledWith({ error: 'MCP server "unknown-server" not found' });
    });

    it('should handle unregistration errors gracefully', async () => {
      createMCPRouter(mockMCPRegistry, mockSkillRegistry);
      const handler = routerMock.delete.mock.calls.find((call: any[]) => call[0] === '/:name')[1];

      req.params = { name: 'test-server' };
      mockMCPRegistry.unregisterClient.mockRejectedValueOnce(new Error('Unregistration failed'));

      await handler(req as Request, res as Response);

      expect(statusMock).toHaveBeenCalledWith(500);
      expect(jsonMock).toHaveBeenCalledWith({ error: 'Unregistration failed' });
    });
  });
});
