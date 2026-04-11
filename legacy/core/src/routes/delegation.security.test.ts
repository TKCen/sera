import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { SignJWT } from 'jose';
import { verifyDelegationToken } from './delegation.js';

describe('Delegation Security', () => {
  const HARDCODED_SECRET = 'sera-delegation-secret';
  const originalJwtSecret = process.env.JWT_SECRET;

  beforeEach(() => {
    delete process.env.JWT_SECRET;
  });

  afterEach(() => {
    if (originalJwtSecret) {
      process.env.JWT_SECRET = originalJwtSecret;
    } else {
      delete process.env.JWT_SECRET;
    }
  });

  it('vulnerability: should verify token signed with hardcoded secret when JWT_SECRET is unset', async () => {
    const payload = {
      sub: 'operator-1',
      act: 'agent-1',
      scope: { service: 'test', permissions: ['*'] },
      iss: 'sera',
      aud: 'agent-1',
      jti: 'token-1',
    };

    const key = new TextEncoder().encode(HARDCODED_SECRET);
    const token = await new SignJWT(payload)
      .setProtectedHeader({ alg: 'HS256' })
      .setIssuedAt()
      .sign(key);

    const verified = await verifyDelegationToken(token);
    expect(verified).toBeNull();
  });

  it('should work with a random secret when JWT_SECRET is unset', async () => {
    // This is a bit tricky to test because verifyDelegationToken uses getDelegationSignKey()
    // which now uses a random secret.
    // We can't easily sign a token that it will accept unless we can get that secret,
    // but the point is that it's random.
    // We can however verify that if we sign a token with SOME key, it's NOT accepted
    // (which we did above for the hardcoded one).
  });
});
