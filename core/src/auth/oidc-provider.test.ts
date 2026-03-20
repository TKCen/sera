import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock jose before importing OIDCAuthPlugin
vi.mock('jose', () => ({
  createRemoteJWKSet: vi.fn(() => Symbol('jwks')),
  jwtVerify: vi.fn(),
}));

import { jwtVerify, createRemoteJWKSet } from 'jose';
import { OIDCAuthPlugin } from './oidc-provider.js';
import type { Request } from 'express';

function makeReq(token?: string): Partial<Request> {
  return {
    headers: token ? { authorization: `Bearer ${token}` } : {},
  };
}

// Minimal fake JWT (3 dot-separated segments)
const FAKE_JWT = 'header.payload.sig';

describe('OIDCAuthPlugin', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    process.env['OIDC_ISSUER_URL'] = 'https://auth.example.com';
    process.env['OIDC_CLIENT_ID'] = 'sera-web';
    process.env['OIDC_AUDIENCE'] = 'sera-api';
    process.env['OIDC_GROUPS_CLAIM'] = 'groups';
    process.env['OIDC_ROLE_MAPPING'] = '{"sera-admins":"admin","sera-ops":"operator"}';
  });

  it('returns null when no Authorization header', async () => {
    const plugin = new OIDCAuthPlugin();
    const result = await plugin.authenticate(makeReq() as Request);
    expect(result).toBeNull();
  });

  it('returns null for API key tokens (sera_ prefix)', async () => {
    const plugin = new OIDCAuthPlugin();
    const result = await plugin.authenticate(makeReq('sera_abc123') as Request);
    expect(result).toBeNull();
  });

  it('returns null for session tokens (sess_ prefix)', async () => {
    const plugin = new OIDCAuthPlugin();
    const result = await plugin.authenticate(makeReq('sess_abc123') as Request);
    expect(result).toBeNull();
  });

  it('returns null for tokens without 3 parts', async () => {
    const plugin = new OIDCAuthPlugin();
    const result = await plugin.authenticate(makeReq('notajwt') as Request);
    expect(result).toBeNull();
  });

  it('extracts OperatorIdentity from valid JWT payload', async () => {
    vi.mocked(jwtVerify).mockResolvedValueOnce({
      payload: {
        sub: 'user-123',
        email: 'alice@example.com',
        name: 'Alice',
        groups: ['sera-admins'],
        iss: 'https://auth.example.com',
        aud: 'sera-api',
        iat: 1000,
        exp: 9999999999,
      },
      protectedHeader: { alg: 'RS256' },
    } as any);

    const plugin = new OIDCAuthPlugin();
    const result = await plugin.authenticate(makeReq(FAKE_JWT) as Request);

    expect(result).toMatchObject({
      sub: 'user-123',
      email: 'alice@example.com',
      name: 'Alice',
      roles: ['admin'],
      authMethod: 'oidc',
    });
  });

  it('maps multiple groups to roles', async () => {
    vi.mocked(jwtVerify).mockResolvedValueOnce({
      payload: {
        sub: 'user-456',
        groups: ['sera-admins', 'sera-ops'],
        iss: 'https://auth.example.com',
        aud: 'sera-api',
        iat: 1000,
        exp: 9999999999,
      },
      protectedHeader: { alg: 'RS256' },
    } as any);

    const plugin = new OIDCAuthPlugin();
    const result = await plugin.authenticate(makeReq(FAKE_JWT) as Request);
    expect(result?.roles).toContain('admin');
    expect(result?.roles).toContain('operator');
  });

  it('defaults to viewer role when no matching groups', async () => {
    vi.mocked(jwtVerify).mockResolvedValueOnce({
      payload: {
        sub: 'user-789',
        groups: ['some-other-group'],
        iss: 'https://auth.example.com',
        aud: 'sera-api',
        iat: 1000,
        exp: 9999999999,
      },
      protectedHeader: { alg: 'RS256' },
    } as any);

    const plugin = new OIDCAuthPlugin();
    const result = await plugin.authenticate(makeReq(FAKE_JWT) as Request);
    expect(result?.roles).toEqual(['viewer']);
  });

  it('throws on expired token', async () => {
    const expiredErr = new Error('JWT expired') as any;
    expiredErr.code = 'ERR_JWT_EXPIRED';
    vi.mocked(jwtVerify).mockRejectedValueOnce(expiredErr);

    const plugin = new OIDCAuthPlugin();
    await expect(plugin.authenticate(makeReq(FAKE_JWT) as Request)).rejects.toThrow(
      'invalid_token'
    );
  });

  it('throws on invalid signature', async () => {
    const sigErr = new Error('signature verification failed') as any;
    sigErr.code = 'ERR_JWS_SIGNATURE_VERIFICATION_FAILED';
    vi.mocked(jwtVerify).mockRejectedValueOnce(sigErr);

    const plugin = new OIDCAuthPlugin();
    await expect(plugin.authenticate(makeReq(FAKE_JWT) as Request)).rejects.toThrow(
      'invalid_token'
    );
  });

  it('does not log the raw token value', async () => {
    const logSpy = vi.spyOn(console, 'log');
    vi.mocked(jwtVerify).mockResolvedValueOnce({
      payload: {
        sub: 'user-123',
        groups: [],
        iss: 'https://auth.example.com',
        aud: 'sera-api',
        iat: 1000,
        exp: 9999999999,
      },
      protectedHeader: { alg: 'RS256' },
    } as any);

    const plugin = new OIDCAuthPlugin();
    await plugin.authenticate(makeReq(FAKE_JWT) as Request);
    for (const call of logSpy.mock.calls) {
      expect(JSON.stringify(call)).not.toContain(FAKE_JWT);
    }
  });
});
