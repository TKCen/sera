import { Router } from 'express';
import { ApiKeyService } from '../auth/api-key-service.js';
import { AuditService } from '../audit/index.js';
import type { WebSessionStore } from '../auth/index.js';
import type { OperatorIdentity, OperatorRole } from '../auth/index.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('AuthRouter');

/**
 * Returns two Express routers:
 * - `publicAuthRouter`: login, oidc/callback, logout — no auth middleware required
 * - `protectedAuthRouter`: /me, api-keys — requires authenticated operator
 */
export function createAuthRouter(sessionStore?: WebSessionStore): {
  publicAuthRouter: Router;
  protectedAuthRouter: Router;
} {
  const publicRouter = Router();
  const protectedRouter = Router();

  // ── GET /api/auth/oidc-config ─────────────────────────────────────────────
  // Public endpoint for CLI device flow discovery.
  publicRouter.get('/oidc-config', (_req, res) => {
    const issuerUrl = process.env.OIDC_ISSUER_URL;
    if (!issuerUrl) {
      res.status(503).json({ error: 'OIDC not configured' });
      return;
    }
    res.json({
      issuerUrl,
      clientId: process.env.OIDC_CLIENT_ID ?? 'sera-web',
    });
  });

  // ── GET /api/auth/login ───────────────────────────────────────────────────
  publicRouter.get('/login', (req, res) => {
    const issuerUrl = process.env.OIDC_ISSUER_URL;
    if (!issuerUrl) {
      res.status(503).json({ error: 'OIDC not configured. Use API key authentication.' });
      return;
    }

    const clientId = process.env.OIDC_CLIENT_ID ?? 'sera-web';
    const webOrigin = process.env.WEB_ORIGIN ?? 'http://localhost:5173';
    const redirectUri = `${webOrigin}/auth/callback`;

    // Standard OIDC authorization endpoint (works for most IdPs)
    const baseUrl = issuerUrl.replace(/\/$/, '');
    const authEndpoint = baseUrl.includes('/realms/')
      ? `${baseUrl}/protocol/openid-connect/auth`
      : `${baseUrl}/authorization`;

    const params = new URLSearchParams({
      response_type: 'code',
      client_id: clientId,
      redirect_uri: redirectUri,
      scope: 'openid profile email',
    });

    res.redirect(`${authEndpoint}?${params.toString()}`);
  });

  // ── POST /api/auth/oidc/callback ─────────────────────────────────────────
  // Web SPA PKCE flow: web sends { code, codeVerifier, redirectUri }.
  // sera-core exchanges with IdP server-side, returns { user, sessionToken }.
  // The OIDC access token is stored server-side only — never sent to the client.
  publicRouter.post('/oidc/callback', async (req, res) => {
    const issuerUrl = process.env.OIDC_ISSUER_URL;
    if (!issuerUrl) {
      res.status(503).json({ error: 'OIDC not configured' });
      return;
    }

    const {
      code,
      codeVerifier,
      redirectUri,
      clientId: reqClientId,
    } = req.body as {
      code?: string;
      codeVerifier?: string;
      redirectUri?: string;
      clientId?: string;
    };

    if (!code || !codeVerifier || !redirectUri) {
      res.status(400).json({ error: 'code, codeVerifier, and redirectUri are required' });
      return;
    }

    const effectiveClientId = reqClientId ?? process.env.OIDC_CLIENT_ID ?? 'sera-web';
    const clientSecret = process.env.OIDC_CLIENT_SECRET ?? '';

    try {
      const tokenUrl = resolveTokenEndpoint(issuerUrl);
      const bodyParams = new URLSearchParams({
        grant_type: 'authorization_code',
        code,
        redirect_uri: redirectUri,
        code_verifier: codeVerifier,
        client_id: effectiveClientId,
      });
      if (clientSecret) bodyParams.set('client_secret', clientSecret);

      const tokenResp = await fetch(tokenUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: bodyParams.toString(),
      });

      if (!tokenResp.ok) {
        const errBody = await tokenResp.text();
        logger.warn(`OIDC token exchange failed (${tokenResp.status})`);
        // Do NOT log errBody as it may contain sensitive data; just log status
        void errBody;
        res.status(401).json({ error: 'Token exchange failed' });
        return;
      }

      const tokens = (await tokenResp.json()) as {
        access_token: string;
        refresh_token?: string;
        expires_in?: number;
        id_token?: string;
      };

      // Parse identity from ID token (preferred) or access token
      const identity = decodeTokenClaims(tokens.id_token ?? tokens.access_token);
      if (!identity) {
        res.status(401).json({ error: 'Could not extract identity from tokens' });
        return;
      }

      const accessTokenExpiry = tokens.expires_in
        ? new Date(Date.now() + tokens.expires_in * 1000)
        : undefined;

      if (!sessionStore) {
        res.status(503).json({ error: 'Session store not initialised' });
        return;
      }

      const sessionToken = sessionStore.create(identity, {
        accessToken: tokens.access_token,
        ...(tokens.refresh_token !== undefined ? { refreshToken: tokens.refresh_token } : {}),
        ...(accessTokenExpiry !== undefined ? { accessTokenExpiry } : {}),
      });

      // HttpOnly session cookie (for same-domain / production setups)
      const maxAge = parseInt(process.env.SESSION_MAX_AGE_SECONDS ?? '28800', 10);
      res.cookie('sera_session', sessionToken, {
        httpOnly: true,
        secure: process.env.NODE_ENV === 'production',
        sameSite: 'strict',
        maxAge: maxAge * 1000,
        path: '/',
      });

      // Return opaque session token (NOT the OIDC access token) + user identity
      res.json({ user: identity, sessionToken });
    } catch (err: unknown) {
      logger.error('OIDC callback error:', err instanceof Error ? err.message : String(err));
      res.status(500).json({ error: 'Authentication failed' });
    }
  });

  // ── POST /api/auth/logout ─────────────────────────────────────────────────
  publicRouter.post('/logout', (req, res) => {
    const cookieSession = parseCookieValue(req.headers.cookie ?? '', 'sera_session');
    const bearerSession = req.headers.authorization?.startsWith('Bearer sess_')
      ? req.headers.authorization.slice(7)
      : undefined;
    const sessionToken = cookieSession ?? bearerSession;

    if (sessionToken && sessionStore) {
      sessionStore.delete(sessionToken);
    }

    res.clearCookie('sera_session', { path: '/' });
    res.json({ loggedOut: true });
  });

  // ── GET /api/auth/me ──────────────────────────────────────────────────────
  protectedRouter.get('/me', (req, res) => {
    if (!req.operator) {
      res.status(401).json({ error: 'Not authenticated as operator' });
      return;
    }
    res.json(req.operator);
  });

  // ── API Key management (Story 16.3) ──────────────────────────────────────

  protectedRouter.get('/api-keys', async (req, res) => {
    try {
      const keys = await ApiKeyService.listKeys(req.operator!.sub);
      res.json(keys);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  protectedRouter.post('/api-keys', async (req, res) => {
    try {
      const { name, roles, expiresInDays } = req.body;
      if (!name) {
        res.status(400).json({ error: 'Name is required' });
        return;
      }

      const keyRoles = (roles as OperatorRole[] | undefined) ?? ['viewer'];
      const result = await ApiKeyService.createKey({
        name,
        ownerSub: req.operator!.sub,
        roles: keyRoles,
        expiresInDays,
      });

      await AuditService.getInstance()
        .record({
          actorType: 'operator',
          actorId: req.operator!.sub,
          actingContext: null,
          eventType: 'api-key.created',
          payload: { keyId: result.metadata.id, name, roles: keyRoles },
        })
        .catch(() => {});

      res.status(201).json(result);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  protectedRouter.delete('/api-keys/:id', async (req, res) => {
    try {
      const revoked = await ApiKeyService.revokeKey(req.params.id, req.operator!.sub);
      if (!revoked) {
        res.status(404).json({ error: 'API key not found or already revoked' });
        return;
      }

      await AuditService.getInstance()
        .record({
          actorType: 'operator',
          actorId: req.operator!.sub,
          actingContext: null,
          eventType: 'api-key.revoked',
          payload: { keyId: req.params.id },
        })
        .catch(() => {});

      res.json({ message: 'API key revoked' });
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  return { publicAuthRouter: publicRouter, protectedAuthRouter: protectedRouter };
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function resolveTokenEndpoint(issuerUrl: string): string {
  const base = issuerUrl.replace(/\/$/, '');
  // Keycloak / Authentik pattern
  if (base.includes('/realms/') || base.includes('/application/o/')) {
    return `${base}/protocol/openid-connect/token`;
  }
  // Generic OIDC — typically /token relative to issuer
  return `${base}/token`;
}

function parseCookieValue(cookieHeader: string, name: string): string | undefined {
  for (const pair of cookieHeader.split(';')) {
    const trimmed = pair.trim();
    const eqIdx = trimmed.indexOf('=');
    if (eqIdx === -1) continue;
    if (trimmed.slice(0, eqIdx).trim() === name) {
      return trimmed.slice(eqIdx + 1).trim();
    }
  }
  return undefined;
}

function decodeTokenClaims(token: string): OperatorIdentity | null {
  try {
    const parts = token.split('.');
    if (parts.length !== 3) return null;
    const payload = JSON.parse(Buffer.from(parts[1]!, 'base64url').toString('utf8')) as Record<
      string,
      unknown
    >;

    const sub = typeof payload['sub'] === 'string' ? payload['sub'] : null;
    if (!sub) return null;

    const groupsClaim = process.env.OIDC_GROUPS_CLAIM ?? 'groups';
    const rawGroups = payload[groupsClaim];
    const groups = Array.isArray(rawGroups) ? (rawGroups as string[]) : [];
    const roles = mapGroupsToRoles(groups);

    const identity: OperatorIdentity = { sub, roles, authMethod: 'oidc' };
    if (typeof payload['email'] === 'string') identity.email = payload['email'];
    if (typeof payload['name'] === 'string') identity.name = payload['name'];
    if (!identity.name && typeof payload['preferred_username'] === 'string') {
      identity.name = payload['preferred_username'];
    }
    return identity;
  } catch {
    return null;
  }
}

function mapGroupsToRoles(groups: string[]): OperatorRole[] {
  let roleMapping: Record<string, OperatorRole> = {};
  try {
    if (process.env.OIDC_ROLE_MAPPING) {
      roleMapping = JSON.parse(process.env.OIDC_ROLE_MAPPING) as typeof roleMapping;
    }
  } catch {
    /* ignore */
  }

  const roles = new Set<OperatorRole>();
  for (const g of groups) {
    const mapped = roleMapping[g];
    if (mapped) roles.add(mapped);
  }
  if (roles.size === 0) roles.add('viewer');
  return Array.from(roles);
}
