import { Router } from 'express';
import { MCPRegistry } from '../mcp/registry.js';
import { SkillRegistry } from '../skills/SkillRegistry.js';
import { requireRole } from '../auth/authMiddleware.js';

export function createMCPRouter(mcpRegistry: MCPRegistry, _skillRegistry: SkillRegistry): Router {
  const router = Router();

  /**
   * GET /api/mcp-servers
   * List all registered MCP servers with status and tool counts.
   */
  router.get('/', requireRole(['admin', 'operator']), async (_req, res) => {
    try {
      const servers = await mcpRegistry.listServers();
      res.json(servers);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/mcp-servers/:name
   * Get details of a specific MCP server including its tools.
   */
  router.get('/:name', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const name = String(req.params['name']);
      const client = mcpRegistry.getClient(name);
      if (!client) {
        return res.status(404).json({ error: `MCP server "${name}" not found` });
      }
      const tools = await client.listTools();
      const servers = await mcpRegistry.listServers();
      const serverInfo = servers.find((s) => s.name === name);
      res.json({
        name,
        status: serverInfo?.status ?? 'unknown',
        toolCount: serverInfo?.toolCount ?? 0,
        tools: tools.tools,
      });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/mcp-servers/:name/health
   * Health check for a specific MCP server.
   */
  router.get('/:name/health', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const name = String(req.params['name']);
      const client = mcpRegistry.getClient(name);
      if (!client) {
        return res.status(404).json({ error: `MCP server "${name}" not found` });
      }
      const tools = await client.listTools();
      res.json({
        name,
        healthy: true,
        toolCount: tools.tools.length,
        checkedAt: new Date().toISOString(),
      });
    } catch (err: unknown) {
      res.json({
        name: String(req.params['name']),
        healthy: false,
        error: (err as Error).message,
        checkedAt: new Date().toISOString(),
      });
    }
  });

  /**
   * POST /api/mcp-servers/:name/reload
   * Reconnect to an MCP server and refresh its tool list.
   */
  router.post('/:name/reload', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const name = String(req.params['name']);
      const client = mcpRegistry.getClient(name);
      if (!client) {
        return res.status(404).json({ error: `MCP server "${name}" not found` });
      }
      await client.disconnect();
      await client.connect();
      const tools = await client.listTools();
      res.json({
        message: `MCP server "${req.params.name}" reloaded`,
        toolCount: tools.tools.length,
      });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * POST /api/mcp-servers
   * Register a new containerized MCP server from manifest.
   */
  router.post('/', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const manifest = req.body;
      if (!manifest || !manifest.metadata || !manifest.metadata.name) {
        return res.status(400).json({ error: 'Invalid manifest: missing metadata.name' });
      }
      await mcpRegistry.registerContainerServer(manifest);
      res.json({ message: `MCP server "${manifest.metadata.name}" registered successfully` });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * DELETE /api/mcp-servers/:name
   * Unregister an MCP server.
   */
  router.delete('/:name', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const name = String(req.params['name']);
      const success = await mcpRegistry.unregisterClient(name);
      if (success) {
        res.json({ message: `MCP server "${name}" unregistered successfully` });
      } else {
        res.status(404).json({ error: `MCP server "${name}" not found` });
      }
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}
