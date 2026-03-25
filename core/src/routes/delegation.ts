/**
 * Delegation & Service Identity Routes — Epic 17
 *
 * Stories covered:
 *   17.2 — agent_service_identities CRUD
 *   17.3 — operator-to-agent delegation (issue JWT, store token)
 *   17.4 — agent-to-subagent delegation (scope intersection, child tokens)
 *   17.6 — delegation revocation with optional cascade
 *   17.7 — delegation audit query endpoint
 */

import { Router } from 'express';
import { v4 as uuidv4 } from 'uuid';
import { SignJWT, jwtVerify } from 'jose';
import { pool } from '../lib/database.js';
import { requireRole } from '../auth/authMiddleware.js';
import { asyncHandler } from '../middleware/asyncHandler.js';
import { AuditService } from '../audit/AuditService.js';
import { IntercomService } from '../intercom/IntercomService.js';
import type { DelegationScope } from '../identity/acting-context.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('DelegationRouter');

// ── In-memory revocation blocklist ──────────────────────────────────────────
// Maps delegationTokenId → expiry timestamp (ms). Checked on every tool call.

const revocationBlocklist = new Map<string, number>();

export function isRevoked(tokenId: string): boolean {
  const expiry = revocationBlocklist.get(tokenId);
  if (expiry === undefined) return false;
  if (Date.now() > expiry) {
    revocationBlocklist.delete(tokenId);
    return false;
  }
  return true;
}

function addToBlocklist(tokenId: string, expiresAt: Date | null): void {
  const ttl = expiresAt ? expiresAt.getTime() : Date.now() + 24 * 60 * 60 * 1000;
  revocationBlocklist.set(tokenId, ttl);
}

// ── JWT signing key (shared with IdentityService) ────────────────────────────

function getDelegationSignKey(): Uint8Array {
  const secret = process.env['JWT_SECRET'] ?? 'sera-delegation-secret';
  return new TextEncoder().encode(secret);
}

// ── Route factory ────────────────────────────────────────────────────────────

export function createDelegationRouter(intercomService?: IntercomService) {
  const router = Router();
  const audit = AuditService.getInstance();

  // ══════════════════════════════════════════════════════════════════════════
  // Story 17.3 — Operator-to-agent delegation
  // ══════════════════════════════════════════════════════════════════════════

  /**
   * POST /api/delegation/issue
   * Issue a delegation token for a specific agent.
   * Requires OIDC-authenticated operator.
   */
  router.post(
    '/issue',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const operator = req.operator!;
      const {
        agentId,
        service,
        permissions,
        resourceConstraints,
        credentialSecretName,
        grantType = 'session',
        expiresAt,
        instanceScoped = false,
        instanceId,
      } = req.body as {
        agentId: string;
        service: string;
        permissions: string[];
        resourceConstraints?: Record<string, string[]>;
        credentialSecretName: string;
        grantType?: 'one-time' | 'session' | 'persistent';
        expiresAt?: string;
        instanceScoped?: boolean;
        instanceId?: string;
      };

      if (!agentId || !service || !permissions || !credentialSecretName) {
        return res
          .status(400)
          .json({ error: 'agentId, service, permissions, and credentialSecretName are required' });
      }

      const scope: DelegationScope = {
        service,
        permissions,
        ...(resourceConstraints !== undefined ? { resourceConstraints } : {}),
      };

      const id = uuidv4();
      const issuedAt = new Date();
      const expiryDate = expiresAt ? new Date(expiresAt) : null;

      // Sign delegation JWT (jose HS256)
      let signedToken: string;
      const jwtBuilder = new SignJWT({
        sub: operator.sub,
        act: instanceId ?? agentId,
        scope,
        iss: 'sera',
        aud: instanceId ?? agentId,
        jti: id,
      })
        .setProtectedHeader({ alg: 'HS256' })
        .setIssuedAt();

      if (expiryDate) {
        signedToken = await jwtBuilder.setExpirationTime(expiryDate).sign(getDelegationSignKey());
      } else {
        signedToken = await jwtBuilder.sign(getDelegationSignKey());
      }

      // Persist to DB
      await pool.query(
        `INSERT INTO delegation_tokens
       (id, principal_type, principal_id, principal_name,
        actor_agent_id, actor_instance_id, scope, grant_type,
        credential_secret_name, signed_token, issued_at, expires_at)
       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)`,
        [
          id,
          'operator',
          operator.sub,
          operator.email ?? operator.sub,
          agentId,
          instanceScoped && instanceId ? instanceId : null,
          JSON.stringify(scope),
          grantType,
          credentialSecretName,
          signedToken,
          issuedAt,
          expiryDate,
        ]
      );

      // Audit
      await audit
        .record({
          actorType: 'operator',
          actorId: operator.sub,
          actingContext: null,
          eventType: 'delegation.created',
          payload: { delegationId: id, agentId, service, grantType },
        })
        .catch((err) => logger.error('Audit failed:', err));

      // Notify agent via Centrifugo
      if (intercomService) {
        const targetChannel = instanceId ? `agent:${instanceId}` : `agent:${agentId}`;
        intercomService
          .publish(targetChannel, {
            type: 'system.delegation-granted',
            delegationTokenId: id,
            signedToken,
            scope,
            grantType,
            expiresAt: expiryDate?.toISOString(),
          })
          .catch(() => {});
      }

      res.status(201).json({ id, signedToken, scope, grantType, issuedAt: issuedAt.toISOString() });
    })
  );

  /**
   * GET /api/delegation
   * List the authenticated operator's active delegations.
   */
  router.get(
    '/',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const operator = req.operator!;
      const { status } = req.query as { status?: string };

      let whereExtra = '';
      if (status === 'active') {
        whereExtra = `AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())`;
      } else if (status === 'revoked') {
        whereExtra = `AND revoked_at IS NOT NULL`;
      } else if (status === 'expired') {
        whereExtra = `AND expires_at IS NOT NULL AND expires_at <= now() AND revoked_at IS NULL`;
      }

      const { rows } = await pool.query(
        `SELECT id, actor_agent_id, actor_instance_id, scope, grant_type,
                issued_at, expires_at, revoked_at, last_used_at, use_count,
                parent_delegation_id,
                CASE
                  WHEN revoked_at IS NOT NULL THEN 'revoked'
                  WHEN expires_at IS NOT NULL AND expires_at <= now() THEN 'expired'
                  ELSE 'active'
                END AS status
         FROM delegation_tokens
         WHERE principal_id = $1 ${whereExtra}
         ORDER BY issued_at DESC`,
        [operator.sub]
      );

      res.json(rows);
    })
  );

  /**
   * DELETE /api/delegation/:id
   * Revoke a delegation token. ?cascade=true also revokes child tokens.
   */
  router.delete(
    '/:id',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const operator = req.operator!;
      const id = req.params['id'] as string;
      const cascade = req.query['cascade'] === 'true';

      // Validate UUID format to prevent raw SQL errors leaking to clients
      const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
      if (!UUID_RE.test(id)) {
        return res.status(400).json({ error: 'Invalid delegation token ID' });
      }

      // Fetch token (must belong to this operator)
      const { rows } = await pool.query(
        `SELECT * FROM delegation_tokens WHERE id = $1 AND principal_id = $2`,
        [id, operator.sub]
      );
      if (rows.length === 0) {
        return res.status(404).json({ error: 'Delegation token not found' });
      }

      const token = rows[0]!;
      const now = new Date();

      let childTokensRevoked = 0;

      if (cascade) {
        // Recursively find and revoke all child tokens
        childTokensRevoked = await revokeChildrenCascade(id, now, intercomService);
      }

      // Revoke the token itself
      await pool.query(`UPDATE delegation_tokens SET revoked_at = $1 WHERE id = $2`, [now, id]);

      addToBlocklist(id, token.expires_at ? new Date(token.expires_at) : null);

      // Notify affected agent
      if (intercomService) {
        const targetChannel = token.actor_instance_id
          ? `agent:${token.actor_instance_id}`
          : `agent:${token.actor_agent_id}`;
        intercomService
          .publish(targetChannel, {
            type: 'system.delegation-revoked',
            delegationTokenId: id,
            service: (token.scope as DelegationScope).service,
            revokedBy: operator.sub,
          })
          .catch(() => {});
      }

      await audit
        .record({
          actorType: 'operator',
          actorId: operator.sub,
          actingContext: null,
          eventType: 'delegation.revoked',
          payload: { delegationId: id, cascade, childTokensRevoked, revokedBy: operator.sub },
        })
        .catch((err) => logger.error('Audit failed:', err));

      res.json({ revoked: true, cascade, childTokensRevoked });
    })
  );

  /**
   * GET /api/delegation/:id/children
   * List all child delegation tokens derived from a parent (admin only).
   */
  router.get(
    '/:id/children',
    requireRole(['admin']),
    asyncHandler(async (req, res) => {
      const { id } = req.params;
      const { rows } = await pool.query(
        `SELECT id, actor_agent_id, actor_instance_id, scope, grant_type,
                issued_at, expires_at, revoked_at, use_count
         FROM delegation_tokens
         WHERE parent_delegation_id = $1
         ORDER BY issued_at DESC`,
        [id]
      );
      res.json(rows);
    })
  );

  /**
   * GET /api/delegation/:id/audit
   * All audit records attributed to a specific delegation token.
   */
  router.get(
    '/:id/audit',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const { id } = req.params;
      const { limit = 50, offset = 0 } = req.query as { limit?: string; offset?: string };

      const { rows } = await pool.query(
        `SELECT * FROM audit_trail
         WHERE acting_context->>'delegationTokenId' = $1
         ORDER BY sequence DESC
         LIMIT $2 OFFSET $3`,
        [id, parseInt(String(limit), 10), parseInt(String(offset), 10)]
      );

      res.json(rows);
    })
  );

  // ══════════════════════════════════════════════════════════════════════════
  // Story 17.2 — Agent service identities
  // ══════════════════════════════════════════════════════════════════════════

  /**
   * GET /api/agents/:agentId/service-identities
   */
  router.get(
    '/agents/:agentId/service-identities',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const { agentId } = req.params;
      const { rows } = await pool.query(
        `SELECT id, agent_scope, service, external_id, display_name, scopes,
                created_at, rotated_at, expires_at, revoked_at
         FROM agent_service_identities
         WHERE (agent_scope = $1 OR agent_scope = $2 OR agent_scope = '*')
           AND revoked_at IS NULL
         ORDER BY created_at DESC`,
        [agentId, agentId]
      );
      res.json(rows);
    })
  );

  /**
   * POST /api/agents/:agentId/service-identities
   */
  router.post(
    '/agents/:agentId/service-identities',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const { agentId } = req.params;
      const {
        service,
        externalId,
        displayName,
        credentialSecretName,
        scopes,
        agentScope,
        expiresAt,
      } = req.body as {
        service: string;
        externalId?: string;
        displayName?: string;
        credentialSecretName: string;
        scopes?: string[];
        agentScope?: string;
        expiresAt?: string;
      };

      if (!service || !credentialSecretName) {
        return res.status(400).json({ error: 'service and credentialSecretName are required' });
      }

      const id = uuidv4();
      const resolvedScope = agentScope ?? agentId;

      await pool.query(
        `INSERT INTO agent_service_identities
         (id, agent_scope, service, external_id, display_name, credential_secret_name, scopes, expires_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)`,
        [
          id,
          resolvedScope,
          service,
          externalId ?? null,
          displayName ?? null,
          credentialSecretName,
          scopes ?? null,
          expiresAt ? new Date(expiresAt) : null,
        ]
      );

      res.status(201).json({
        id,
        agentScope: resolvedScope,
        service,
        externalId,
        displayName,
        scopes,
      });
    })
  );

  /**
   * DELETE /api/agents/:agentId/service-identities/:identityId
   */
  router.delete(
    '/agents/:agentId/service-identities/:identityId',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const { agentId, identityId } = req.params;

      const { rowCount } = await pool.query(
        `UPDATE agent_service_identities
         SET revoked_at = now()
         WHERE id = $1 AND (agent_scope = $2 OR agent_scope = '*')
           AND revoked_at IS NULL`,
        [identityId, agentId]
      );

      if (!rowCount || rowCount === 0) {
        return res.status(404).json({ error: 'Service identity not found' });
      }

      res.json({ revoked: true });
    })
  );

  /**
   * POST /api/agents/:agentId/service-identities/:identityId/rotate
   * Update the credential_secret_name reference after rotating a bot token.
   */
  router.post(
    '/agents/:agentId/service-identities/:identityId/rotate',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const { agentId, identityId } = req.params;
      const { credentialSecretName } = req.body as { credentialSecretName: string };

      if (!credentialSecretName) {
        return res.status(400).json({ error: 'credentialSecretName is required' });
      }

      const { rowCount } = await pool.query(
        `UPDATE agent_service_identities
         SET credential_secret_name = $1, rotated_at = now()
         WHERE id = $2 AND (agent_scope = $3 OR agent_scope = '*')
           AND revoked_at IS NULL`,
        [credentialSecretName, identityId, agentId]
      );

      if (!rowCount || rowCount === 0) {
        return res.status(404).json({ error: 'Service identity not found' });
      }

      res.json({ rotated: true });
    })
  );

  /**
   * GET /api/agents/:agentId/delegations
   * List active inbound delegations for an agent (Epic 17).
   */
  router.get(
    '/agents/:agentId/delegations',
    requireRole(['admin', 'operator']),
    asyncHandler(async (req, res) => {
      const { agentId } = req.params;
      const { rows } = await pool.query(
        `SELECT id, principal_id, principal_name, scope, grant_type,
                issued_at, expires_at, last_used_at, use_count,
                CASE
                  WHEN revoked_at IS NOT NULL THEN 'revoked'
                  WHEN expires_at IS NOT NULL AND expires_at <= now() THEN 'expired'
                  ELSE 'active'
                END AS status
         FROM delegation_tokens
         WHERE (actor_agent_id = $1 OR actor_instance_id::text = $1)
           AND revoked_at IS NULL
           AND (expires_at IS NULL OR expires_at > now())
         ORDER BY issued_at DESC`,
        [agentId]
      );
      res.json(rows);
    })
  );

  return router;
}

// ── Cascade revocation helper ─────────────────────────────────────────────

async function revokeChildrenCascade(
  parentId: string,
  now: Date,
  intercomService?: IntercomService
): Promise<number> {
  const { rows } = await pool.query(
    `SELECT id, actor_agent_id, actor_instance_id, scope, expires_at
     FROM delegation_tokens
     WHERE parent_delegation_id = $1 AND revoked_at IS NULL`,
    [parentId]
  );

  let count = 0;
  for (const child of rows) {
    // Recurse first
    count += await revokeChildrenCascade(child.id as string, now, intercomService);

    // Revoke this child
    await pool.query(`UPDATE delegation_tokens SET revoked_at = $1 WHERE id = $2`, [now, child.id]);
    addToBlocklist(child.id as string, child.expires_at ? new Date(child.expires_at) : null);

    if (intercomService) {
      const targetChannel = child.actor_instance_id
        ? `agent:${child.actor_instance_id}`
        : `agent:${child.actor_agent_id}`;
      intercomService
        .publish(targetChannel, {
          type: 'system.delegation-revoked',
          delegationTokenId: child.id,
          service: (child.scope as DelegationScope).service,
          reason: 'parent-revoked',
        })
        .catch(() => {});
    }

    count++;
  }

  return count;
}

// ── JWT verification helper (used by auth middleware extension) ───────────

export async function verifyDelegationToken(
  token: string
): Promise<{ sub: string; act: string; scope: DelegationScope; jti: string } | null> {
  try {
    const { payload } = await jwtVerify(token, getDelegationSignKey(), {
      algorithms: ['HS256'],
      issuer: 'sera',
    });
    return payload as unknown as { sub: string; act: string; scope: DelegationScope; jti: string };
  } catch {
    return null;
  }
}

// ── Background job: expire delegation tokens ─────────────────────────────

export async function expireOldDelegationTokens(): Promise<void> {
  try {
    const { rows } = await pool.query(
      `UPDATE delegation_tokens
       SET revoked_at = expires_at
       WHERE expires_at IS NOT NULL
         AND expires_at <= now()
         AND revoked_at IS NULL
       RETURNING id, expires_at`
    );
    for (const row of rows) {
      addToBlocklist(row.id as string, new Date(row.expires_at));
    }
    if (rows.length > 0) {
      logger.info(`Expired ${rows.length} delegation token(s)`);
    }
  } catch (err) {
    logger.error('Failed to expire delegation tokens:', err);
  }
}
