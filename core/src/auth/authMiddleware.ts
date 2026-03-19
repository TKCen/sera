/**
 * Auth middleware — protects internal SERA routes.
 *
 * Authentication priority order:
 *   1. Operator AuthService plugins (API key, OIDC JWT)
 *   2. Web session token (sess_* prefix — opaque, validated against WebSessionStore)
 *   3. Agent JWT (internal jose-signed token issued by IdentityService at spawn)
 */

import type { Request, Response, NextFunction } from 'express';
import type { IdentityService } from './IdentityService.js';
import type { AgentTokenClaims } from './types.js';
import type { OperatorIdentity, OperatorRole } from './interfaces.js';
import type { AuthService } from './auth-service.js';
import type { WebSessionStore } from './web-session-store.js';

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

export function createAuthMiddleware(
  identityService: IdentityService,
  authService: AuthService,
  sessionStore?: WebSessionStore,
) {
  return async (req: Request, res: Response, next: NextFunction): Promise<void> => {
    const authHeader = req.headers.authorization;

    // Check session cookie (httpOnly, set after OIDC callback)
    const cookieHeader = req.headers.cookie ?? '';
    const sessionCookie = parseCookieValue(cookieHeader, 'sera_session');
    const sessionToken = sessionCookie ?? (authHeader?.startsWith('Bearer sess_') ? authHeader.slice(7) : undefined);

    // Try session token first (web UI sessions)
    if (sessionToken && sessionStore) {
      const session = sessionStore.get(sessionToken);
      if (session) {
        req.operator = session.identity;
        next();
        return;
      }
    }

    if (!authHeader || !authHeader.startsWith('Bearer ')) {
      res.status(401).json({ error: 'Missing or malformed Authorization header' });
      return;
    }

    const token = authHeader.slice(7);

    try {
      // 1. Try operator authentication (API keys and OIDC JWTs)
      const operator = await authService.authenticate(req);
      if (operator) {
        req.operator = operator;
        next();
        return;
      }

      // 2. Try agent authentication (internal JWTs)
      const claims = await identityService.verifyToken(token);
      req.agentIdentity = claims;
      next();
    } catch (err: any) {
      const message =
        err.code === 'ERR_JWT_EXPIRED' || err.name === 'TokenExpiredError'
          ? 'Token expired'
          : err.message || 'Invalid credentials';
      res.status(401).json({ error: message });
    }
  };
}

// ── Role-Based Access Control ───────────────────────────────────────────────

export function requireRole(roles: OperatorRole[]) {
  return (req: Request, res: Response, next: NextFunction): void => {
    if (!req.operator) {
      res.status(403).json({ error: 'Operator access required' });
      return;
    }

    const hasRole = req.operator.roles.some(
      (role: OperatorRole) => roles.includes(role) || role === 'admin',
    );

    if (!hasRole) {
      res.status(403).json({ error: 'Insufficient permissions' });
      return;
    }

    next();
  };
}

// ── Helpers ─────────────────────────────────────────────────────────────────

function parseCookieValue(cookieHeader: string, name: string): string | undefined {
  for (const pair of cookieHeader.split(';')) {
    const trimmed = pair.trim();
    const eqIdx = trimmed.indexOf('=');
    if (eqIdx === -1) continue;
    const key = trimmed.slice(0, eqIdx).trim();
    if (key === name) return trimmed.slice(eqIdx + 1).trim();
  }
  return undefined;
}
