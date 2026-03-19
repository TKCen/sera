/**
 * Auth types for the SERA Identity Provider.
 *
 * JWTs are issued to agent containers on spawn and used to authenticate
 * requests to internal services (LLM Proxy, Centrifugo, etc.).
 */

// ── Token Payload (claims we embed) ─────────────────────────────────────────

export interface AgentTokenPayload {
  /** Unique agent instance ID. */
  agentId: string;
  /** Human-readable agent name. */
  agentName: string;
  /** Circle scope the agent belongs to. */
  circleId: string;
  /** Capability gates granted to this agent (e.g. 'internet-access'). */
  capabilities: string[];
  /**
   * Token scope — 'agent' for agent containers, 'internal' for sera-core's
   * own internal calls. Proxy rejects anything other than these two values.
   */
  scope: 'agent' | 'internal';
}

// ── Decoded Token (payload + standard JWT fields) ───────────────────────────

export interface AgentTokenClaims extends AgentTokenPayload {
  iat: number;
  exp: number;
}
