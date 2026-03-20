import { Router } from 'express';
import { MCPRegistry } from '../mcp/registry.js';
import { SkillRegistry } from '../skills/SkillRegistry.js';

export function createMCPRouter(mcpRegistry: MCPRegistry, _skillRegistry: SkillRegistry) {
  const router = Router();

  /**
   * POST /api/mcp-servers
   * Register a new containerized MCP server from manifest.
   */
  router.post('/', async (req, res) => {
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
  router.delete('/:name', async (req, res) => {
    try {
      const success = await mcpRegistry.unregisterClient(req.params.name);
      if (success) {
        res.json({ message: `MCP server "${req.params.name}" unregistered successfully` });
      } else {
        res.status(404).json({ error: `MCP server "${req.params.name}" not found` });
      }
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}
