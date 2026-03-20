/**
 * CredentialResolver — selects the correct credential for a tool call based
 * on the current acting context.
 *
 * Resolution order (first match wins):
 *   1. Active delegation token in actingContext.delegationTokenId
 *   2. Agent service identity matching the service for this agent
 *   3. Named secret in SecretsProvider with allowedAgents including this agent
 *   4. null — no credential available
 *
 * Story 17.5
 */

import { Logger } from '../lib/logger.js';
import { pool } from '../lib/database.js';
import { SecretsManager } from '../secrets/secrets-manager.js';
import type { ActingContext } from './acting-context.js';

const logger = new Logger('CredentialResolver');

export type CredentialSource = 'delegation' | 'service-identity' | 'secret';

export interface ResolvedCredential {
  value: string;
  sourceType: CredentialSource;
  sourceId: string; // delegationTokenId | identityId | secretName (never the value)
}

export class CredentialResolver {
  /**
   * Resolve a credential for a given service and agent context.
   * Returns null if no credential is available.
   *
   * @param service        Service identifier (e.g. 'github', 'jira')
   * @param agentId        Agent template ID or instance ID
   * @param instanceId     Agent instance UUID
   * @param actingContext  Current acting context (may be null for anonymous calls)
   */
  async resolve(
    service: string,
    agentId: string,
    instanceId: string,
    actingContext: ActingContext | null
  ): Promise<ResolvedCredential | null> {
    // 1. Active delegation token
    if (actingContext?.delegationTokenId) {
      const result = await this.resolveFromDelegation(
        service,
        actingContext.delegationTokenId,
        agentId,
        instanceId
      );
      if (result) {
        logger.debug(
          `[${agentId}] resolved "${service}" via delegation ${actingContext.delegationTokenId}`
        );
        return result;
      }
    }

    // 2. Agent service identity (instance > template > '*')
    const identityResult = await this.resolveFromServiceIdentity(service, agentId, instanceId);
    if (identityResult) {
      logger.debug(
        `[${agentId}] resolved "${service}" via service-identity ${identityResult.sourceId}`
      );
      return identityResult;
    }

    // 3. Named secret with matching tag or allowedAgents
    const secretResult = await this.resolveFromSecret(service, agentId);
    if (secretResult) {
      logger.debug(`[${agentId}] resolved "${service}" via secret ${secretResult.sourceId}`);
      return secretResult;
    }

    // 4. No match
    logger.debug(`[${agentId}] no credential found for service "${service}"`);
    return null;
  }

  // ── Resolution steps ───────────────────────────────────────────────────────

  private async resolveFromDelegation(
    service: string,
    delegationTokenId: string,
    agentId: string,
    instanceId: string
  ): Promise<ResolvedCredential | null> {
    const { rows } = await pool.query(
      `SELECT * FROM delegation_tokens
       WHERE id = $1
         AND (actor_agent_id = $2 OR actor_instance_id::text = $3)
         AND revoked_at IS NULL
         AND (expires_at IS NULL OR expires_at > now())`,
      [delegationTokenId, agentId, instanceId]
    );

    const token = rows[0];
    if (!token) return null;

    const scope = token.scope as { service: string; permissions: string[] };
    if (scope.service !== '*' && scope.service !== service) return null;

    const secretsManager = SecretsManager.getInstance();
    const value = await secretsManager.get(token.credential_secret_name, { agentId });
    if (!value) return null;

    // Increment use_count; revoke immediately for one-time grants
    if (token.grant_type === 'one-time') {
      await pool.query(
        `UPDATE delegation_tokens
         SET use_count = use_count + 1, last_used_at = now(), revoked_at = now()
         WHERE id = $1`,
        [delegationTokenId]
      );
    } else {
      await pool.query(
        `UPDATE delegation_tokens SET use_count = use_count + 1, last_used_at = now() WHERE id = $1`,
        [delegationTokenId]
      );
    }

    return { value, sourceType: 'delegation', sourceId: delegationTokenId };
  }

  private async resolveFromServiceIdentity(
    service: string,
    agentId: string,
    instanceId: string
  ): Promise<ResolvedCredential | null> {
    // Priority: instance UUID > template name > wildcard '*'
    const { rows } = await pool.query(
      `SELECT * FROM agent_service_identities
       WHERE service = $1
         AND revoked_at IS NULL
         AND (expires_at IS NULL OR expires_at > now())
         AND (agent_scope = $2 OR agent_scope = $3 OR agent_scope = '*')
       ORDER BY
         CASE agent_scope
           WHEN $2 THEN 1
           WHEN $3 THEN 2
           ELSE 3
         END
       LIMIT 1`,
      [service, instanceId, agentId]
    );

    const identity = rows[0];
    if (!identity) return null;

    const secretsManager = SecretsManager.getInstance();
    const value = await secretsManager.get(identity.credential_secret_name, { agentId });
    if (!value) return null;

    return { value, sourceType: 'service-identity', sourceId: identity.id as string };
  }

  private async resolveFromSecret(
    service: string,
    agentId: string
  ): Promise<ResolvedCredential | null> {
    const secretsManager = SecretsManager.getInstance();
    const secrets = await secretsManager.list({ agentId, tags: [service] }, { agentId });

    const matching = secrets.find(
      (s) => s.allowedAgents.includes(agentId) || s.allowedAgents.includes('*')
    );
    if (!matching) return null;

    const value = await secretsManager.get(matching.name, { agentId });
    if (!value) return null;

    return { value, sourceType: 'secret', sourceId: matching.name };
  }
}
