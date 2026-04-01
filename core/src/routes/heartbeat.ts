/**
 * Heartbeat Route — receives heartbeat pings from agent containers.
 *
 * Agent containers send periodic heartbeats to prove they're alive.
 * The Orchestrator tracks these to detect unhealthy instances.
 *
 * Endpoints:
 *   POST /api/agents/:id/heartbeat — record heartbeat from container
 *   GET  /api/agents/health        — list unhealthy instances
 */

import { Router } from 'express';
import type { Request, Response } from 'express';
import type { HeartbeatService } from '../agents/HeartbeatService.js';
import type { IdentityService } from '../auth/IdentityService.js';
import type { AuthService } from '../auth/auth-service.js';
import { createAuthMiddleware } from '../auth/authMiddleware.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('Heartbeat');

export function createHeartbeatRouter(
  heartbeatService: HeartbeatService,
  identityService: IdentityService,
  authService: AuthService
): Router {
  const router = Router();
  const authMiddleware = createAuthMiddleware(identityService, authService);

  /**
   * POST /:id/heartbeat — agent container reports it's alive
   * Protected by JWT auth (same token as LLM proxy).
   */
  router.post('/:id/heartbeat', authMiddleware, (req: Request, res: Response) => {
    const { id } = req.params;
    const identity = req.agentIdentity!;

    // Validate the JWT's agentId matches the URL param
    if (identity.agentId !== id) {
      logger.warn(`Heartbeat identity mismatch: JWT=${identity.agentId} URL=${id}`);
      res.status(403).json({ error: 'Token agentId does not match URL' });
      return;
    }

    heartbeatService.registerHeartbeat(id);
    logger.debug(`Heartbeat received from ${id}`);

    res.json({ status: 'ok', timestamp: new Date().toISOString() });
  });

  /**
   * GET /health — return instances that haven't heartbeated recently
   * No auth required (internal admin API).
   */
  router.get('/health', (_req: Request, res: Response) => {
    const unhealthy = heartbeatService.getUnhealthyInstances();
    res.json({
      unhealthy: unhealthy.map((u) => ({
        instanceId: u.instanceId,
        lastSeen: u.lastSeen.toISOString(),
      })),
    });
  });

  return router;
}
