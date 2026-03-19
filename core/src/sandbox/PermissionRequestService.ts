/**
 * PermissionRequestService — human-in-the-loop grant requests.
 * Story 3.9
 *
 * Agents request runtime access to resources outside their capability set.
 * Requests are published to Centrifugo and held in memory. The operator
 * responds via the decision endpoint. The agent is notified asynchronously —
 * we do NOT block the agent HTTP thread waiting for approval.
 *
 * DECISION: Non-blocking design per task instructions. Emit event and return
 * `{ status: 'pending', requestId }` immediately. Agent polls grants or
 * subscribes to Centrifugo for notification.
 */

import { v4 as uuidv4 } from 'uuid';
import { Logger } from '../lib/logger.js';
import type { AgentRegistry } from '../agents/registry.service.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import { ChannelNamespace } from '../intercom/ChannelNamespace.js';

const logger = new Logger('PermissionRequestService');

// ── Types ────────────────────────────────────────────────────────────────────

export interface PermissionRequest {
  requestId: string;
  agentId: string;
  agentName: string;
  dimension: 'filesystem' | 'network' | 'exec.commands';
  value: string;
  reason: string | undefined;
  requestedAt: string;
  status: 'pending' | 'granted' | 'denied' | 'expired';
}

export interface PermissionDecision {
  decision: 'grant' | 'deny';
  grantType?: 'one-time' | 'session' | 'persistent';
  expiresAt?: string;
}

export interface SessionGrant {
  grantId: string;
  agentInstanceId: string;
  dimension: string;
  value: string;
  grantType: 'one-time' | 'session';
  expiresAt: string | undefined;
  grantedAt: string;
}

// ── Service ──────────────────────────────────────────────────────────────────

export class PermissionRequestService {
  /** Pending requests keyed by requestId */
  private pending = new Map<string, PermissionRequest>();

  /** Session grants keyed by agentInstanceId → grant list */
  private sessionGrants = new Map<string, SessionGrant[]>();

  constructor(
    private registry: AgentRegistry,
    private intercom: IntercomService,
  ) {}

  // ── Request ──────────────────────────────────────────────────────────────

  /**
   * Receive a permission request from an agent.
   * Publishes to Centrifugo immediately and returns a pending status.
   * Does NOT block — the agent should poll GET /api/agents/:id/grants or
   * subscribe to Centrifugo for notification.
   */
  async request(
    agentId: string,
    agentName: string,
    dimension: PermissionRequest['dimension'],
    value: string,
    reason?: string,
  ): Promise<{ status: 'pending'; requestId: string }> {
    const requestId = uuidv4();
    const permRequest: PermissionRequest = {
      requestId,
      agentId,
      agentName,
      dimension,
      value,
      reason,
      requestedAt: new Date().toISOString(),
      status: 'pending',
    };

    this.pending.set(requestId, permRequest);

    // Publish to Centrifugo system.permission-requests channel — non-blocking
    this.intercom.publishSystemEvent('permission-requests', {
      requestId,
      agentId,
      agentName,
      dimension,
      value,
      reason,
      requestedAt: permRequest.requestedAt,
    }).catch(err => logger.warn('Failed to publish permission request to Centrifugo:', err));

    // Auto-expire after timeout
    const timeoutMs = parseInt(process.env.PERMISSION_REQUEST_TIMEOUT_MS ?? String(5 * 60 * 1000), 10);
    setTimeout(() => {
      const still = this.pending.get(requestId);
      if (still && still.status === 'pending') {
        still.status = 'expired';
        this.pending.delete(requestId);
        this.intercom.publishSystemEvent('permission-requests', {
          requestId,
          agentId,
          status: 'expired',
          expiredAt: new Date().toISOString(),
        }).catch(() => {});
        logger.info(`Permission request ${requestId} expired`);
      }
    }, timeoutMs);

    logger.info(`Permission request ${requestId} from ${agentName} (${agentId}): ${dimension}=${value}`);
    return { status: 'pending', requestId };
  }

  // ── Decision ─────────────────────────────────────────────────────────────

  /**
   * Record the operator's grant/deny decision.
   * For session grants: stored in memory map.
   * For persistent grants: inserted into capability_grants table.
   */
  async decide(
    requestId: string,
    decision: PermissionDecision,
    operatorId?: string,
  ): Promise<PermissionRequest> {
    const req = this.pending.get(requestId);
    if (!req) {
      throw new Error(`Permission request ${requestId} not found or already decided`);
    }

    req.status = decision.decision === 'grant' ? 'granted' : 'denied';
    this.pending.delete(requestId);

    if (decision.decision === 'grant') {
      const grantType = decision.grantType ?? 'one-time';

      if (grantType === 'session') {
        const grant: SessionGrant = {
          grantId: uuidv4(),
          agentInstanceId: req.agentId,
          dimension: req.dimension,
          value: req.value,
          grantType: 'session',
          expiresAt: decision.expiresAt,
          grantedAt: new Date().toISOString(),
        };
        const existing = this.sessionGrants.get(req.agentId) ?? [];
        existing.push(grant);
        this.sessionGrants.set(req.agentId, existing);
        logger.info(`Session grant stored for agent ${req.agentId}: ${req.dimension}=${req.value}`);
      } else if (grantType === 'persistent') {
        await this.registry.createCapabilityGrant({
          agentInstanceId: req.agentId,
          dimension: req.dimension,
          value: req.value,
          grantType: 'persistent',
          ...(operatorId !== undefined ? { grantedBy: operatorId } : {}),
          ...(decision.expiresAt !== undefined ? { expiresAt: decision.expiresAt } : {}),
        });
        logger.info(`Persistent grant stored for agent ${req.agentId}: ${req.dimension}=${req.value}`);
      }
      // one-time: nothing persisted — just returned to caller
    }

    // Notify agent via Centrifugo (agent specific channel, using system.* prefix for notification if appropriate, 
    // or status. In this case, Story 9.6 says system.* for platform events. 
    // Agent-specific notification can be on agent:{agentId}:status or a specific event.
    // The previous implementation used `agent.${req.agentId}.grants` which is not in our canonical list.
    // I'll use system.permission-decisions or similar if it's a platform event.
    this.intercom.publishSystemEvent('permission-decisions', {
      requestId,
      agentId: req.agentId,
      decision: decision.decision,
      grantType: decision.grantType,
      dimension: req.dimension,
      value: req.value,
      decidedAt: new Date().toISOString(),
    }).catch(() => {});

    logger.info(`Permission request ${requestId} decided: ${decision.decision} (${decision.grantType ?? 'n/a'}) by ${operatorId ?? 'unknown'}`);
    return req;
  }

  // ── Query ────────────────────────────────────────────────────────────────

  listPending(agentId?: string): PermissionRequest[] {
    const all = Array.from(this.pending.values());
    return agentId ? all.filter(r => r.agentId === agentId) : all;
  }

  getSessionGrants(agentInstanceId: string): SessionGrant[] {
    return this.sessionGrants.get(agentInstanceId) ?? [];
  }

  /**
   * Remove session grants for an agent — called when container stops (Story 3.9).
   */
  clearSessionGrants(agentInstanceId: string): void {
    this.sessionGrants.delete(agentInstanceId);
  }

  /**
   * Remove a session grant by ID (Story 3.10 revoke path).
   */
  revokeSessionGrant(agentInstanceId: string, grantId: string): boolean {
    const grants = this.sessionGrants.get(agentInstanceId);
    if (!grants) return false;
    const idx = grants.findIndex(g => g.grantId === grantId);
    if (idx === -1) return false;
    grants.splice(idx, 1);
    return true;
  }

  /**
   * Check if an agent has a session or one-time grant for a specific dimension/value.
   * Used by the file proxy in Story 3.10.
   */
  hasActiveGrant(agentInstanceId: string, dimension: string, value: string): boolean {
    const grants = this.sessionGrants.get(agentInstanceId) ?? [];
    return grants.some(g => {
      if (g.dimension !== dimension) return false;
      if (g.expiresAt && new Date(g.expiresAt) < new Date()) return false;
      // Value match: exact or prefix (for filesystem paths)
      return g.value === value || value.startsWith(g.value);
    });
  }
}
