import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createMCPRouter } from './mcp.js';

vi.mock('../auth/authMiddleware.js', () => ({
  requireRole: vi.fn(() => (req: any, res: any, next: any) => next()),
}));

import type { MCPRegistry } from '../mcp/registry.js';
import type { SkillRegistry } from '../skills/SkillRegistry.js';

describe('MCP Routes', () => {
  let app: express.Express;
  let mcpRegistryMock: {
    listServers: ReturnType<typeof vi.fn>;
    getClient: ReturnType<typeof vi.fn>;
    registerContainerServer: ReturnType<typeof vi.fn>;
    unregisterClient: ReturnType<typeof vi.fn>;
  };
  let skillRegistryMock: Partial<SkillRegistry>;

  beforeEach(() => {
    mcpRegistryMock = {
      listServers: vi.fn().mockResolvedValue([
        { name: 'github-mcp', status: 'connected', toolCount: 5 },
        { name: 'web-search', status: 'connected', toolCount: 2 },
      ]),
      getClient: vi.fn(),
      registerContainerServer: vi.fn().mockResolvedValue({}),
      unregisterClient: vi.fn().mockResolvedValue(true),
    };

    skillRegistryMock = {};

    app = express();
    app.use(express.json());
    app.use(
      '/api/mcp-servers',
      createMCPRouter(
        mcpRegistryMock as unknown as MCPRegistry,
        skillRegistryMock as unknown as SkillRegistry
      )
    );
  });

  describe('GET /api/mcp-servers', () => {
    it('returns list of servers', async () => {
      const res = await request(app).get('/api/mcp-servers');
      expect(res.status).toBe(200);
      expect(res.body).toHaveLength(2);
      expect(res.body[0]).toEqual({
        name: 'github-mcp',
        status: 'connected',
        toolCount: 5,
      });
    });

    it('returns 500 on error', async () => {
      mcpRegistryMock.listServers.mockRejectedValueOnce(new Error('DB down'));
      const res = await request(app).get('/api/mcp-servers');
      expect(res.status).toBe(500);
      expect(res.body.error).toBe('DB down');
    });
  });

  describe('GET /api/mcp-servers/:name', () => {
    it('returns server details with tools', async () => {
      const mockClient = {
        listTools: vi.fn().mockResolvedValue({
          tools: [
            { name: 'create_pr', description: 'Create a pull request' },
            { name: 'list_issues', description: 'List issues' },
          ],
        }),
      };
      mcpRegistryMock.getClient.mockReturnValue(mockClient);

      const res = await request(app).get('/api/mcp-servers/github-mcp');
      expect(res.status).toBe(200);
      expect(res.body.name).toBe('github-mcp');
      expect(res.body.status).toBe('connected');
      expect(res.body.tools).toHaveLength(2);
    });

    it('returns 404 for unknown server', async () => {
      mcpRegistryMock.getClient.mockReturnValue(undefined);
      const res = await request(app).get('/api/mcp-servers/nonexistent');
      expect(res.status).toBe(404);
    });
  });

  describe('GET /api/mcp-servers/:name/health', () => {
    it('returns healthy status', async () => {
      const mockClient = {
        listTools: vi.fn().mockResolvedValue({ tools: [{ name: 't1' }] }),
      };
      mcpRegistryMock.getClient.mockReturnValue(mockClient);

      const res = await request(app).get('/api/mcp-servers/github-mcp/health');
      expect(res.status).toBe(200);
      expect(res.body.healthy).toBe(true);
      expect(res.body.toolCount).toBe(1);
      expect(res.body.checkedAt).toBeDefined();
    });

    it('returns unhealthy on error', async () => {
      const mockClient = {
        listTools: vi.fn().mockRejectedValue(new Error('Connection refused')),
      };
      mcpRegistryMock.getClient.mockReturnValue(mockClient);

      const res = await request(app).get('/api/mcp-servers/github-mcp/health');
      expect(res.status).toBe(200);
      expect(res.body.healthy).toBe(false);
      expect(res.body.error).toBe('Connection refused');
    });

    it('returns 404 for unknown server', async () => {
      mcpRegistryMock.getClient.mockReturnValue(undefined);
      const res = await request(app).get('/api/mcp-servers/nonexistent/health');
      expect(res.status).toBe(404);
    });
  });

  describe('POST /api/mcp-servers/:name/reload', () => {
    it('reconnects and returns tool count', async () => {
      const mockClient = {
        disconnect: vi.fn().mockResolvedValue(undefined),
        connect: vi.fn().mockResolvedValue(undefined),
        listTools: vi.fn().mockResolvedValue({ tools: [{ name: 't1' }, { name: 't2' }] }),
      };
      mcpRegistryMock.getClient.mockReturnValue(mockClient);

      const res = await request(app).post('/api/mcp-servers/github-mcp/reload');
      expect(res.status).toBe(200);
      expect(res.body.toolCount).toBe(2);
      expect(mockClient.disconnect).toHaveBeenCalled();
      expect(mockClient.connect).toHaveBeenCalled();
    });

    it('returns 404 for unknown server', async () => {
      mcpRegistryMock.getClient.mockReturnValue(undefined);
      const res = await request(app).post('/api/mcp-servers/nonexistent/reload');
      expect(res.status).toBe(404);
    });
  });

  describe('POST /api/mcp-servers', () => {
    it('registers a server from manifest', async () => {
      const manifest = {
        metadata: { name: 'new-server' },
        image: 'mcp/new-server:latest',
        transport: 'stdio',
      };
      const res = await request(app).post('/api/mcp-servers').send(manifest);
      expect(res.status).toBe(200);
      expect(res.body.message).toContain('new-server');
      expect(mcpRegistryMock.registerContainerServer).toHaveBeenCalledWith(manifest);
    });

    it('returns 400 for invalid manifest', async () => {
      const res = await request(app).post('/api/mcp-servers').send({ invalid: true });
      expect(res.status).toBe(400);
    });
  });

  describe('DELETE /api/mcp-servers/:name', () => {
    it('unregisters a server', async () => {
      const res = await request(app).delete('/api/mcp-servers/github-mcp');
      expect(res.status).toBe(200);
      expect(res.body.message).toContain('github-mcp');
    });

    it('returns 404 for unknown server', async () => {
      mcpRegistryMock.unregisterClient.mockResolvedValueOnce(false);
      const res = await request(app).delete('/api/mcp-servers/nonexistent');
      expect(res.status).toBe(404);
    });
  });
});
