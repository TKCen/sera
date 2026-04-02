import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { rateLimitStub } from './rateLimitStub.js';
import type { Request, Response, NextFunction } from 'express';
import type { AgentTokenClaims } from '../auth/types.js';
import type { OperatorIdentity } from '../auth/interfaces.js';

describe('rateLimitStub', () => {
  let req: Partial<Request> & { agentIdentity?: AgentTokenClaims; operator?: OperatorIdentity };
  let res: Partial<Response>;
  let next: NextFunction;

  beforeEach(() => {
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
    // Since buckets are stored in a module-level Map, we need a way to clear them or use unique IDs.
    // For these tests, we'll use unique IDs.
  });

  it('sets rate limit headers with default limits and calls next', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));
    req.agentIdentity = { agentId: 'agent-1', agentName: 'A1', circleId: 'C1', capabilities: [], scope: 'agent', iat: 0, exp: 0 };

    rateLimitStub(req as Request, res as Response, next);

    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Limit', 60);
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 59);
    expect(next).toHaveBeenCalledOnce();
  });

  it('enforces rate limits and returns 429 when exceeded', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));
    const agentId = 'agent-limit-test';
    req.agentIdentity = { agentId, agentName: 'A', circleId: 'C', capabilities: [], scope: 'agent', iat: 0, exp: 0 };

    // Consume all 60 tokens
    for (let i = 0; i < 60; i++) {
      rateLimitStub(req as Request, res as Response, next);
    }
    expect(next).toHaveBeenCalledTimes(60);
    vi.mocked(next).mockClear();

    // 61st request should fail
    rateLimitStub(req as Request, res as Response, next);
    expect(res.status).toHaveBeenCalledWith(429);
    expect(res.json).toHaveBeenCalledWith(expect.objectContaining({ error: 'rate_limit_exceeded' }));
    expect(next).not.toHaveBeenCalled();
  });

  it('refills tokens over time', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));
    const agentId = 'agent-refill-test';
    req.agentIdentity = { agentId, agentName: 'A', circleId: 'C', capabilities: [], scope: 'agent', iat: 0, exp: 0 };

    // Consume all 60 tokens
    for (let i = 0; i < 60; i++) {
      rateLimitStub(req as Request, res as Response, next);
    }
    vi.mocked(next).mockClear();

    // Advance time by 30 seconds -> should refill 30 tokens
    vi.advanceTimersByTime(30000);

    rateLimitStub(req as Request, res as Response, next);
    expect(next).toHaveBeenCalledOnce();
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 29);
  });

  it('uses different limits for operators', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));
    req.operator = { sub: 'operator-1', roles: [], authMethod: 'api-key' };

    rateLimitStub(req as Request, res as Response, next);

    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Limit', 120);
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 119);
    expect(next).toHaveBeenCalledOnce();
  });

  it('isolates buckets by identifier', () => {
    const now = 1600000000000;
    vi.setSystemTime(new Date(now));

    const req1 = { agentIdentity: { agentId: 'agent-A' } } as any;
    const req2 = { agentIdentity: { agentId: 'agent-B' } } as any;

    // Consume all tokens for agent-A
    for (let i = 0; i < 60; i++) {
      rateLimitStub(req1 as Request, res as Response, next);
    }

    // agent-B should still have all tokens
    vi.mocked(res.setHeader).mockClear();
    rateLimitStub(req2 as Request, res as Response, next);
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 59);
  });
});
