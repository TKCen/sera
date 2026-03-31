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
    };
    next = vi.fn();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('sets rate limit headers with default limits and calls next', () => {
    const now = 1600000000000; // Some timestamp
    vi.setSystemTime(new Date(now));

    rateLimitStub(req as Request, res as Response, next);

    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Limit', 60);
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Remaining', 60);
    expect(res.setHeader).toHaveBeenCalledWith('X-RateLimit-Reset', Math.floor(now / 1000) + 60);
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
      exp: 1234567890
    };

    rateLimitStub(req as Request, res as Response, next);

    expect(res.setHeader).toHaveBeenCalled();
    expect(next).toHaveBeenCalledOnce();
  });

  it('handles request with operator', () => {
    req.operator = { sub: 'operator-123', roles: [], authMethod: 'api-key' };

    rateLimitStub(req as Request, res as Response, next);

    expect(res.setHeader).toHaveBeenCalled();
    expect(next).toHaveBeenCalledOnce();
  });
});
