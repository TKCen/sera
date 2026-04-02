/**
 * Rate limiting middleware — Story 4.7.
 *
 * This middleware hooks into the LLM proxy request pipeline and sets the
 * standard rate limit response headers.
 *
 * Implemented as a token-bucket limiter per agentId on
 * POST /v1/llm/chat/completions and a per-operator limiter on management
 * endpoints.
 *
 * Configuration structure:
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

/**
 * Token bucket implementation for rate limiting.
 */
class TokenBucket {
  private tokens: number;
  private lastRefill: number;

  constructor(private readonly capacity: number) {
    this.tokens = capacity;
    this.lastRefill = Date.now();
  }

  /**
   * Refills the bucket based on elapsed time.
   */
  private refill(): void {
    const now = Date.now();
    const elapsedMs = now - this.lastRefill;
    const refillAmount = (elapsedMs * this.capacity) / 60000;

    if (refillAmount > 0) {
      this.tokens = Math.min(this.capacity, this.tokens + refillAmount);
      this.lastRefill = now;
    }
  }

  /**
   * Attempts to consume one token.
   * @returns true if a token was consumed, false otherwise.
   */
  consume(): boolean {
    this.refill();
    if (this.tokens >= 1) {
      this.tokens -= 1;
      return true;
    }
    return false;
  }

  getRemaining(): number {
    this.refill();
    return Math.floor(this.tokens);
  }

  getResetTime(): number {
    // Estimate when the bucket will be full
    const missingTokens = this.capacity - this.tokens;
    const msToFull = (missingTokens * 60000) / this.capacity;
    return Math.floor((Date.now() + msToFull) / 1000);
  }
}

const buckets = new Map<string, TokenBucket>();

/**
 * Rate limiting middleware.
 *
 * Sets standard X-RateLimit-* headers and enforces limits.
 */
export function rateLimitStub(req: Request, res: Response, next: NextFunction): void {
  const isOperator = !!req.operator;
  const identifier = req.agentIdentity?.agentId ?? req.operator?.sub ?? 'unknown';
  const limit = isOperator ? OPERATOR_RPM_LIMIT : AGENT_RPM_LIMIT;

  let bucket = buckets.get(identifier);
  if (!bucket) {
    bucket = new TokenBucket(limit);
    buckets.set(identifier, bucket);
  }

  if (bucket.consume()) {
    res.setHeader('X-RateLimit-Limit', limit);
    res.setHeader('X-RateLimit-Remaining', bucket.getRemaining());
    res.setHeader('X-RateLimit-Reset', bucket.getResetTime());
    next();
  } else {
    const resetTime = bucket.getResetTime();
    res.setHeader('X-RateLimit-Limit', limit);
    res.setHeader('X-RateLimit-Remaining', 0);
    res.setHeader('X-RateLimit-Reset', resetTime);
    res.setHeader('Retry-After', 1); // Simple 1-second retry hint
    res.status(429).json({
      error: 'rate_limit_exceeded',
      message: 'Rate limit exceeded. Please try again later.',
    });
  }
}
