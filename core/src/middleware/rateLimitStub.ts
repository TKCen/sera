/**
 * Rate limiting stub middleware — Story 4.7 (deferred).
 *
 * This middleware hooks into the LLM proxy request pipeline and sets the
 * standard rate limit response headers. The enforcement logic is a no-op stub.
 *
 * When implemented, this should be a token-bucket limiter per agentId on
 * POST /v1/llm/chat/completions and a per-operator limiter on management
 * endpoints.
 *
 * Configuration structure (future):
 *   RATE_LIMIT_AGENT_RPM=60      — requests per minute per agent
 *   RATE_LIMIT_OPERATOR_RPM=120  — requests per minute per operator key
 *
 * @see docs/epics/04-llm-proxy-and-governance.md § Story 4.7
 *
 * # TODO: implement rate limiting enforcement — see Epic 04 Story 4.7
 *
 * v1 plan: in-process token bucket per agentId, reset on sera-core restart.
 * Multi-node future: Redis-backed state.
 * Distinguish from budget (Story 4.3): rate limiting = requests/minute,
 * budgets = tokens/period.
 */

import type { Request, Response, NextFunction } from 'express';

// Placeholder limits — will become meaningful once enforcement is implemented
const STUB_AGENT_RPM_LIMIT = parseInt(process.env.RATE_LIMIT_AGENT_RPM ?? '60', 10);

/**
 * No-op rate limiting middleware.
 *
 * Sets standard X-RateLimit-* headers so clients can parse them, but does
 * not enforce any limits. Replace this with a real token-bucket implementation
 * in Epic 04 Story 4.7.
 */
export function rateLimitStub(req: Request, res: Response, next: NextFunction): void {
  const agentId = req.agentIdentity?.agentId ?? req.operator?.sub ?? 'unknown';

  // Set rate limit headers (values are stubs — not enforced)
  res.setHeader('X-RateLimit-Limit', STUB_AGENT_RPM_LIMIT);
  res.setHeader('X-RateLimit-Remaining', STUB_AGENT_RPM_LIMIT); // stub: always full
  res.setHeader('X-RateLimit-Reset', Math.floor(Date.now() / 1000) + 60);

  // # TODO: implement rate limiting enforcement — see Epic 04 Story 4.7
  // When implemented:
  //   1. Look up agentId in in-process token bucket map
  //   2. If bucket is empty, respond 429 with reason: 'rate_limit'
  //   3. Otherwise consume one token and proceed
  void agentId; // suppress lint warning until enforcement is added

  next();
}
