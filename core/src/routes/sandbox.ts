/**
 * Sandbox API routes.
 *
 * REST endpoints for container lifecycle management. All operations
 * require an `agentName` parameter to look up the agent's manifest
 * and enforce RBAC permissions.
 *
 * @see sera/docs/reimplementation/agent-workspace-architecture.md § Sandbox Manager API
 */

import { Router } from 'express';
import type { AgentManifest } from '../agents/manifest/types.js';
import { SandboxManager } from '../sandbox/SandboxManager.js';
import { ToolRunner } from '../sandbox/ToolRunner.js';
import { SubagentRunner } from '../agents/SubagentRunner.js';
import { PolicyViolationError } from '../sandbox/TierPolicy.js';

// ── Factory ─────────────────────────────────────────────────────────────────────

export function createSandboxRouter(
  sandboxManager: SandboxManager,
  resolveManifest: (agentName: string) => AgentManifest | undefined,
): Router {
  const router = Router();
  const toolRunner = new ToolRunner(sandboxManager);
  const subagentRunner = new SubagentRunner(sandboxManager);

  /**
   * Helper: resolve manifest or return 404.
   */
  function getManifestOrFail(agentName: string | undefined, res: any): AgentManifest | null {
    if (!agentName || typeof agentName !== 'string') {
      res.status(400).json({ error: 'agentName is required' });
      return null;
    }
    const manifest = resolveManifest(agentName);
    if (!manifest) {
      res.status(404).json({ error: `Agent "${agentName}" not found` });
      return null;
    }
    return manifest;
  }

  // ── POST /spawn — Spawn a sandbox container ──────────────────────────────

  router.post('/spawn', async (req, res) => {
    try {
      const manifest = getManifestOrFail(req.body.agentName, res);
      if (!manifest) return;

      const { type, image, command, env, workDir, subagentRole, task } = req.body;

      if (!type || !image) {
        return res.status(400).json({ error: 'type and image are required' });
      }

      const result = await sandboxManager.spawn(manifest, {
        agentName: manifest.metadata.name,
        type,
        image,
        command,
        env,
        workDir,
        subagentRole,
        task,
      });

      res.status(201).json(result);
    } catch (err: unknown) {
      if (err instanceof PolicyViolationError) {
        return res.status(403).json({ error: err.message, violation: err.violation });
      }
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── POST /exec — Execute command in a container ──────────────────────────

  router.post('/exec', async (req, res) => {
    try {
      const manifest = getManifestOrFail(req.body.agentName, res);
      if (!manifest) return;

      const { containerId, command } = req.body;

      if (!containerId || !command) {
        return res.status(400).json({ error: 'containerId and command are required' });
      }

      const result = await sandboxManager.exec(manifest, {
        containerId,
        agentName: manifest.metadata.name,
        command,
      });

      res.json(result);
    } catch (err: unknown) {
      if (err instanceof PolicyViolationError) {
        return res.status(403).json({ error: err.message, violation: err.violation });
      }
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── DELETE /:id — Remove a container ─────────────────────────────────────

  router.delete('/:id', async (req, res) => {
    try {
      const manifest = getManifestOrFail(req.query.agentName as string, res);
      if (!manifest) return;

      await sandboxManager.remove(manifest, req.params.id!);
      res.json({ success: true });
    } catch (err: unknown) {
      if (err instanceof PolicyViolationError) {
        return res.status(403).json({ error: err.message, violation: err.violation });
      }
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── GET /:id/logs — Get container logs ───────────────────────────────────

  router.get('/:id/logs', async (req, res) => {
    try {
      const tail = req.query.tail ? parseInt(req.query.tail as string) : undefined;
      const logs = await sandboxManager.getLogs(req.params.id!, tail);
      res.json({ logs });
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── GET / — List all sandbox containers ──────────────────────────────────

  router.get('/', (req, res) => {
    const agentName = req.query.agentName as string | undefined;
    const containers = sandboxManager.listContainers(agentName);
    res.json(containers);
  });

  // ── POST /tool — Run a tool in an ephemeral container ────────────────────

  router.post('/tool', async (req, res) => {
    try {
      const manifest = getManifestOrFail(req.body.agentName, res);
      if (!manifest) return;

      const { toolName, command, image, timeoutSeconds } = req.body;

      if (!toolName || !command) {
        return res.status(400).json({ error: 'toolName and command are required' });
      }

      const result = await toolRunner.runTool(manifest, {
        agentName: manifest.metadata.name,
        toolName,
        command,
        image,
        timeoutSeconds,
      });

      res.json(result);
    } catch (err: unknown) {
      if (err instanceof PolicyViolationError) {
        return res.status(403).json({ error: err.message, violation: err.violation });
      }
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── POST /subagent — Spawn a subagent ────────────────────────────────────

  router.post('/subagent', async (req, res) => {
    try {
      const manifest = getManifestOrFail(req.body.agentName, res);
      if (!manifest) return;

      const { subagentRole, task, image } = req.body;

      if (!subagentRole || !task) {
        return res.status(400).json({ error: 'subagentRole and task are required' });
      }

      const result = await subagentRunner.spawnSubagent(manifest, subagentRole, task, { image });
      res.status(201).json(result);
    } catch (err: unknown) {
      if (err instanceof PolicyViolationError) {
        return res.status(403).json({ error: err.message, violation: err.violation });
      }
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  return router;
}
