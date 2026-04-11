/**
 * Rate limiting middleware — Story 4.7.
 *
 * This middleware hooks into the LLM proxy request pipeline and sets the
 * standard rate limit response headers. It enforces per-agent and per-operator
 * limits using an in-process token bucket.
 *
 * Configuration:
 *   RATE_LIMIT_AGENT_RPM=60      — requests per minute per agent
 *   RATE_LIMIT_OPERATOR_RPM=120  — requests per minute per operator key
 *
 * @see docs/epics/04-llm-proxy-and-governance.md § Story 4.7
 *
 * v1 plan: in-process token bucket per agentId, reset on sera-core restart.
 * Multi-node future: Redis-backed state.
 * Distinguish from budget (Story 4.3): rate limiting = requests/minute,
 * budgets = tokens/period.
 */

import type { Request, Response, NextFunction } from 'express';

const AGENT_RPM_LIMIT = parseInt(process.env.RATE_LIMIT_AGENT_RPM ?? '60', 10);
const OPERATOR_RPM_LIMIT = parseInt(process.env.RATE_LIMIT_OPERATOR_RPM ?? '120', 10);

interface TokenBucket {
  tokens: number;
  lastRefill: number;
  limit: number;
}

const buckets = new Map<string, TokenBucket>();

// Periodic cleanup to avoid memory leaks (every 10 minutes)
setInterval(
  () => {
    const now = Date.now();
    for (const [id, bucket] of buckets.entries()) {
      // If the bucket has been full and inactive for more than 10 minutes, remove it
      if (bucket.tokens >= bucket.limit && now - bucket.lastRefill > 10 * 60 * 1000) {
        buckets.delete(id);
      }
    }
  },
  10 * 60 * 1000
).unref();

/**
 * Rate limiting middleware using a token-bucket algorithm.
 *
 * Sets standard X-RateLimit-* headers and enforces limits.
 * Responds with 429 if the limit is exceeded.
 */
export function rateLimitStub(req: Request, res: Response, next: NextFunction): void {
  const isOperator = !!req.operator;
  const agentId = req.agentIdentity?.agentId ?? req.operator?.sub ?? 'unknown';
  const limit = isOperator ? OPERATOR_RPM_LIMIT : AGENT_RPM_LIMIT;

  const now = Date.now();
  let bucket = buckets.get(agentId);

  if (!bucket) {
    bucket = {
      tokens: limit,
      lastRefill: now,
      limit: limit,
    };
    buckets.set(agentId, bucket);
  } else {
    // Refill tokens: (time elapsed in ms / 60000 ms per minute) * limit
    const elapsed = now - bucket.lastRefill;
    const refill = (elapsed / 60000) * limit;
    bucket.tokens = Math.min(limit, bucket.tokens + refill);
    bucket.lastRefill = now;
  }

  const canProceed = bucket.tokens >= 1;

  // Set rate limit headers
  res.setHeader('X-RateLimit-Limit', limit);
  // If we can't proceed, current remaining tokens are floor(tokens)
  // If we can proceed, remaining tokens after this request will be floor(tokens - 1)
  res.setHeader(
    'X-RateLimit-Remaining',
    Math.floor(canProceed ? bucket.tokens - 1 : bucket.tokens)
  );
  res.setHeader('X-RateLimit-Reset', Math.floor(now / 1000) + 60);

  if (!canProceed) {
    res.status(429).json({
      error: 'Rate limit exceeded',
      reason: 'rate_limit',
      limit: limit,
      resetAt: new Date(now + 60000).toISOString(),
    });
    return;
  }

  // Consume one token
  bucket.tokens -= 1;

  next();
}

/** Exported for testing only */
export function clearRateLimitBuckets(): void {
  buckets.clear();
}
