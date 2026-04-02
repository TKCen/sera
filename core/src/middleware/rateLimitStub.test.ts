import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { rateLimitStub, clearRateLimitBuckets } from './rateLimitStub.js';
import type { Request, Response, NextFunction } from 'express';
import type { AgentTokenClaims } from '../auth/types.js';
import type { OperatorIdentity } from '../auth/interfaces.js';

describe('rateLimitStub', () => {
  let req: Partial<Request> & { agentIdentity?: AgentTokenClaims; operator?: OperatorIdentity };
  let res: Partial<Response>;
  let next: NextFunction;

  beforeEach(() => {
    clearRateLimitBuckets();
    req = {};
    res = {
      setHeader: vi.fn(),
      status: vi.fn().mockReturnThis(),
      json: vi.fn().mockReturnThis(),
    };
    next = vi.fn();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('sets rate limit headers and decrements remaining tokens', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));

    rateLimitStub(req as Request, res as Response, next);

    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Limit', 60);
    // After one request, 60 - 1 = 59
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 59);
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Reset', Math.floor(now / 1000) + 60);
    expect(next).toHaveBeenCalledOnce();
  });

  it('blocks requests when the limit is exceeded (429)', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));

    // Consume all 60 tokens
    for (let i = 0; i < 60; i++) {
      rateLimitStub(req as Request, res as Response, next);
    }
    expect(next).toHaveBeenCalledTimes(60);
    vi.clearAllMocks();

    // 61st request should be blocked
    rateLimitStub(req as Request, res as Response, next);

    expect(res.status).toHaveBeenCalledWith(429);
    expect(res.json).toHaveBeenCalledWith(
      expect.objectContaining({ reason: 'rate_limit' })
    );
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 0);
    expect(next).not.toHaveBeenCalled();
  });

  it('refills tokens over time', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));

    // Consume all 60 tokens
    for (let i = 0; i < 60; i++) {
      rateLimitStub(req as Request, res as Response, next);
    }
    vi.clearAllMocks();

    // Advance time by 30 seconds (should refill 30 tokens)
    vi.setSystemTime(new Date(now + 30000));

    rateLimitStub(req as Request, res as Response, next);

    expect(next).toHaveBeenCalledOnce();
    // 30 refill - 1 consumed = 29 remaining
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 29);
  });

  it('handles operator requests with higher limits', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));

    req.operator = { sub: 'operator-123', roles: [], authMethod: 'api-key' };

    rateLimitStub(req as Request, res as Response, next);

    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Limit', 120);
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 119);
    expect(next).toHaveBeenCalledOnce();
  });

  it('handles request with agentIdentity', () => {
    req.agentIdentity = {
      agentId: 'agent-123',
      agentName: 'TestAgent',
      circleId: 'circle-123',
      capabilities: [],
      scope: 'agent',
      iat: 1234567890,
      exp: 1234567890,
    };

    rateLimitStub(req as Request, res as Response, next);

    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Limit', 60);
    expect(next).toHaveBeenCalledOnce();
  });
});
