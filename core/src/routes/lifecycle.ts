/**
 * Lifecycle Routes — Epic 03 agent lifecycle management endpoints.
 *
 * Covers Stories:
 *   3.2  — POST   /api/agents/:id/resolve-capabilities (dry-run)
 *   3.4  — POST   /api/agents/:id/worktree/merge
 *         DELETE  /api/agents/:id/worktree
 *   3.5  — GET    /api/agents/:id/logs
 *   3.7  — POST   /api/agents/:id/cleanup
 *   3.8  — GET    /api/agents/:id/subagents
 *   3.9  — POST   /api/agents/:id/permission-request
 *         GET    /api/permission-requests
 *         POST   /api/permission-requests/:requestId/decision
 *         GET    /api/agents/:id/grants
 *   3.10 — POST   /api/agents/:id/restart
 *         DELETE  /api/agents/:id/grants/:grantId
 */

import { Router } from 'express';
import type { Request, Response, RequestHandler } from 'express';
import { CapabilityResolver } from '../capability/resolver.js';
import { WorktreeManager } from '../sandbox/WorktreeManager.js';
import type { AgentRegistry } from '../agents/registry.service.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import type { SandboxManager } from '../sandbox/SandboxManager.js';
import { PermissionRequestService } from '../sandbox/PermissionRequestService.js';
import type { PermissionDecision } from '../sandbox/PermissionRequestService.js';

// Typed param shapes

type IdParam = { id: string };
type RequestIdParam = { requestId: string };
type IdGrantParam = { id: string; grantId: string };

export function createLifecycleRouter(
  registry: AgentRegistry,
  orchestrator: Orchestrator,
  sandboxManager: SandboxManager,
  permService: PermissionRequestService
): Router {
  const router = Router();

  // ── Story 3.2: Capability dry-run ─────────────────────────────────────────

  const resolveCapabilities: RequestHandler<IdParam> = async (req, res) => {
    try {
      const resolver = new CapabilityResolver(registry);
      const result = await resolver.resolve(req.params.id);
      res.json(result);
    } catch (err: unknown) {
      const error = err as Error;
      const code = error.name === 'CapabilityEscalationError' ? 422 : 500;
      res.status(code).json({ error: error.message });
    }
  };
  router.post('/:id/resolve-capabilities', resolveCapabilities as RequestHandler);

  // ── Story 3.4: Worktree management ────────────────────────────────────────

  const worktreeMerge: RequestHandler<IdParam> = async (req, res) => {
    try {
      const instance = await registry.getInstance(req.params.id);
      if (!instance) return void res.status(404).json({ error: 'Instance not found' });

      const { repoPath, targetBranch = 'main' } = req.body as {
        repoPath?: string;
        targetBranch?: string;
      };
      if (!repoPath) return void res.status(400).json({ error: 'repoPath is required' });

      WorktreeManager.merge(repoPath, instance.name, instance.id, targetBranch);
      res.json({ merged: true, branch: `agent/${instance.name}/${instance.id}`, targetBranch });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.post('/:id/worktree/merge', worktreeMerge as RequestHandler);

  const worktreeDelete: RequestHandler<IdParam> = async (req, res) => {
    try {
      const instance = await registry.getInstance(req.params.id);
      if (!instance) return void res.status(404).json({ error: 'Instance not found' });

      const { repoPath } = req.body as { repoPath?: string };
      if (!repoPath) return void res.status(400).json({ error: 'repoPath is required' });

      WorktreeManager.remove(repoPath, instance.name, instance.id);
      res.json({ removed: true });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.delete('/:id/worktree', worktreeDelete as RequestHandler);

  // ── Story 3.5: Container logs ─────────────────────────────────────────────

  const getLogs: RequestHandler<IdParam> = async (req, res) => {
    try {
      const instance = await registry.getInstance(req.params.id);
      if (!instance) return void res.status(404).json({ error: 'Instance not found' });
      const container_id: string | undefined = instance.container_id;
      if (!container_id)
        return void res.status(404).json({ error: 'No container for this instance' });

      const tailStr = req.query['tail'];
      const tail = tailStr ? parseInt(tailStr as string, 10) : 100;
      const logs = await sandboxManager.getLogs(container_id, tail);
      res.type('text/plain').send(logs);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.get('/:id/logs', getLogs as RequestHandler);

  // ── Story 3.7: Cleanup ────────────────────────────────────────────────────

  const cleanup: RequestHandler<IdParam> = async (req, res) => {
    try {
      const instance = await registry.getInstance(req.params.id);
      if (!instance) return void res.status(404).json({ error: 'Instance not found' });

      await orchestrator.cleanupInstance(req.params.id);
      res.json({ cleaned: true });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.post('/:id/cleanup', cleanup as RequestHandler);

  // ── Story 3.8 + 3.11: Subagent tree ─────────────────────────────────────

  const getSubagents: RequestHandler<IdParam> = async (req, res) => {
    try {
      const subagents = await registry.listSubagents(req.params.id);
      res.json(subagents);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.get('/:id/subagents', getSubagents as RequestHandler);

  // ── Story 3.9: Permission requests ────────────────────────────────────────

  const permissionRequest: RequestHandler<IdParam> = async (req, res) => {
    try {
      const id = req.params.id;
      const identity = (req as unknown as { agentIdentity?: { agentId?: string } }).agentIdentity;

      if (identity?.agentId && identity.agentId !== id) {
        return void res.status(403).json({ error: 'Token agentId does not match URL' });
      }

      const instance = await registry.getInstance(id);
      if (!instance) return void res.status(404).json({ error: 'Instance not found' });

      const { dimension, value, reason } = req.body as {
        dimension?: 'filesystem' | 'network' | 'exec.commands';
        value?: string;
        reason?: string;
      };

      if (!dimension || !value) {
        return void res.status(400).json({ error: 'dimension and value are required' });
      }

      const result = await permService.request(id, instance.name, dimension, value, reason);
      res.json(result);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.post('/:id/permission-request', permissionRequest as RequestHandler);

  const listGrants: RequestHandler<IdParam> = async (req, res) => {
    try {
      const sessionGrants = permService.getSessionGrants(req.params.id);
      const persistentGrants = await registry.listCapabilityGrants(req.params.id);
      res.json({ session: sessionGrants, persistent: persistentGrants });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.get('/:id/grants', listGrants as RequestHandler);

  // ── Create a capability grant (operator-initiated) ─────────────────────────
  const createGrant: RequestHandler<IdParam> = async (req, res) => {
    try {
      const instance = await registry.getInstance(req.params.id);
      if (!instance) return void res.status(404).json({ error: 'Instance not found' });

      const { dimension, value, grantType, expiresAt } = req.body as {
        dimension?: string;
        value?: string;
        grantType?: 'one-time' | 'session' | 'persistent';
        expiresAt?: string;
      };

      if (!dimension || !value) {
        return void res.status(400).json({ error: 'dimension and value are required' });
      }

      const grant = await registry.createCapabilityGrant({
        agentInstanceId: req.params.id,
        dimension,
        value,
        grantType: grantType ?? 'persistent',
        grantedBy: 'operator',
        ...(expiresAt ? { expiresAt } : {}),
      });

      res.status(201).json(grant);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.post('/:id/grants', createGrant as RequestHandler);

  // ── Story 3.10: Dynamic filesystem + restart ──────────────────────────────

  const restart: RequestHandler<IdParam> = async (req, res) => {
    try {
      const instance = await registry.getInstance(req.params.id);
      if (!instance) return void res.status(404).json({ error: 'Instance not found' });

      if (instance.lifecycle_mode !== 'persistent') {
        return void res.status(409).json({ error: 'Only persistent agents can be restarted' });
      }

      await orchestrator.stopInstance(req.params.id);
      const agent = await orchestrator.startInstance(req.params.id);
      res.json({ restarted: true, agentName: agent.name });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.post('/:id/restart', restart as RequestHandler);

  const revokeGrant: RequestHandler<IdGrantParam> = async (req, res) => {
    try {
      const { id, grantId } = req.params;
      const revokedSession = permService.revokeSessionGrant(id, grantId);
      if (revokedSession) return void res.json({ revoked: true, type: 'session' });

      const revokedPersistent = await registry.revokeCapabilityGrant(grantId);
      if (!revokedPersistent) return void res.status(404).json({ error: 'Grant not found' });

      res.json({ revoked: true, type: 'persistent' });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  };
  router.delete('/:id/grants/:grantId', revokeGrant as RequestHandler);

  return router;
}

// ── Separate router for /api/permission-requests ─────────────────────────────

export function createPermissionRouter(permService: PermissionRequestService): Router {
  const router = Router();

  router.get('/', (req: Request, res: Response) => {
    const agentId = req.query['agentId'] as string | undefined;
    res.json(permService.listPending(agentId));
  });

  const decide: RequestHandler<RequestIdParam> = async (req, res) => {
    try {
      const { requestId } = req.params;
      const { decision, grantType, expiresAt } = req.body as {
        decision?: 'grant' | 'deny';
        grantType?: PermissionDecision['grantType'];
        expiresAt?: string;
      };

      if (!decision || !['grant', 'deny'].includes(decision)) {
        return void res.status(400).json({ error: 'decision must be "grant" or "deny"' });
      }

      const operatorIdentity = (
        req as unknown as { operator?: { sub?: string; email?: string; name?: string } }
      ).operator;
      const result = await permService.decide(
        requestId,
        {
          decision,
          ...(grantType !== undefined ? { grantType } : {}),
          ...(expiresAt !== undefined ? { expiresAt } : {}),
        },
        operatorIdentity?.sub,
        operatorIdentity?.email,
        operatorIdentity?.name
      );
      res.json(result);
    } catch (err: unknown) {
      const error = err as Error;
      const code = error.message.includes('not found') ? 404 : 500;
      res.status(code).json({ error: error.message });
    }
  };
  router.post('/:requestId/decision', decide as RequestHandler);

  return router;
}
