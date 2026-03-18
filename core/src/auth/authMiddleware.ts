/**
 * Auth middleware — protects internal SERA routes with JWT verification.
 *
 * Extracts the Bearer token from the Authorization header, verifies it
 * using the IdentityService, and attaches the decoded claims to the request.
 */

import type { Request, Response, NextFunction } from 'express';
import type { IdentityService } from './IdentityService.js';
import type { AgentTokenClaims } from './types.js';
import type { OperatorIdentity, OperatorRole } from './interfaces.js';
import type { AuthService } from './auth-service.js';

// ── Express type augmentation ───────────────────────────────────────────────

declare global {
  namespace Express {
    interface Request {
      agentIdentity?: AgentTokenClaims;
      operator?: OperatorIdentity;
    }
  }
}

// ── Middleware Factory ──────────────────────────────────────────────────────

/**
 * Creates an Express middleware that verifies SERA identity (agent JWTs or operator credentials).
 * Protected routes will have `req.agentIdentity` or `req.operator` populated on success.
 */
export function createAuthMiddleware(identityService: IdentityService, authService: AuthService) {
  return async (req: Request, res: Response, next: NextFunction): Promise<void> => {
    const authHeader = req.headers.authorization;

    if (!authHeader || !authHeader.startsWith('Bearer ')) {
      res.status(401).json({ error: 'Missing or malformed Authorization header' });
      return;
    }

    const token = authHeader.slice(7); // Remove "Bearer "

    try {
      // 1. Try operator authentication (AuthService handles API keys and future OIDC)
      const operator = await authService.authenticate(req);
      if (operator) {
        req.operator = operator;
        next();
        return;
      }

      // 2. Try agent authentication (internal JWTs)
      const claims = identityService.verifyToken(token);
      req.agentIdentity = claims;
      next();
    } catch (err: any) {
      const message =
        err.name === 'TokenExpiredError'
          ? 'Token expired'
          : err.message || 'Invalid credentials';
      res.status(401).json({ error: message });
    }
  };
}

/**
 * Middleware for Role-Based Access Control.
 * Requires createAuthMiddleware to have run previously.
 */
export function requireRole(roles: OperatorRole[]) {
  return (req: Request, res: Response, next: NextFunction): void => {
    if (!req.operator) {
      res.status(403).json({ error: 'Operator access required' });
      return;
    }

    const hasRole = req.operator.roles.some((role: OperatorRole) =>
      roles.includes(role) || role === 'admin'
    );

    if (!hasRole) {
      res.status(403).json({ error: 'Insufficient permissions' });
      return;
    }

    next();
  };
}
