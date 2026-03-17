/**
 * Auth middleware — protects internal SERA routes with JWT verification.
 *
 * Extracts the Bearer token from the Authorization header, verifies it
 * using the IdentityService, and attaches the decoded claims to the request.
 */

import type { Request, Response, NextFunction } from 'express';
import type { IdentityService } from './IdentityService.js';
import type { AgentTokenClaims } from './types.js';

// ── Express type augmentation ───────────────────────────────────────────────

declare global {
  namespace Express {
    interface Request {
      agentIdentity?: AgentTokenClaims;
    }
  }
}

// ── Middleware Factory ──────────────────────────────────────────────────────

/**
 * Creates an Express middleware that verifies SERA identity JWTs.
 * Protected routes will have `req.agentIdentity` populated on success.
 */
export function createAuthMiddleware(identityService: IdentityService) {
  return (req: Request, res: Response, next: NextFunction): void => {
    const authHeader = req.headers.authorization;

    if (!authHeader || !authHeader.startsWith('Bearer ')) {
      res.status(401).json({ error: 'Missing or malformed Authorization header' });
      return;
    }

    const token = authHeader.slice(7); // Remove "Bearer "

    try {
      const claims = identityService.verifyToken(token);
      req.agentIdentity = claims;
      next();
    } catch (err: any) {
      const message =
        err.name === 'TokenExpiredError'
          ? 'Token expired'
          : 'Invalid token';
      res.status(401).json({ error: message });
    }
  };
}
