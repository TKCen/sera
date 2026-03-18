import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ApiKeyProvider } from './api-key-provider.js';
import * as db from '../lib/database.js';
import argon2 from 'argon2';

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

vi.mock('argon2', () => ({
  default: {
    verify: vi.fn(),
  },
}));

describe('ApiKeyProvider', () => {
  let provider: ApiKeyProvider;
  const BOOTSTRAP_KEY = 'sera_test_bootstrap_123';

  beforeEach(() => {
    vi.clearAllMocks();
    process.env.SERA_BOOTSTRAP_API_KEY = BOOTSTRAP_KEY;
    provider = new ApiKeyProvider();
  });

  it('should authenticate with bootstrap key', async () => {
    const req = {
      headers: {
        authorization: `Bearer ${BOOTSTRAP_KEY}`
      }
    } as any;

    const identity = await provider.authenticate(req);
    expect(identity).not.toBeNull();
    expect(identity?.sub).toBe('system:bootstrap');
    expect(identity?.roles).toContain('admin');
  });

  it('should return null if no authorization header', async () => {
    const req = { headers: {} } as any;
    const identity = await provider.authenticate(req);
    expect(identity).toBeNull();
  });

  it('should return null if not Bearer token', async () => {
    const req = { headers: { authorization: 'Basic ...' } } as any;
    const identity = await provider.authenticate(req);
    expect(identity).toBeNull();
  });

  it('should authenticate with a valid database API key', async () => {
    const key = 'sera_valid_key';
    const req = { headers: { authorization: `Bearer ${key}` } } as any;

    vi.mocked(db.query).mockResolvedValueOnce({
      rowCount: 1,
      rows: [{
        id: 'key-id',
        key_hash: 'hashed_key',
        owner_sub: 'user-123',
        roles: ['operator']
      }]
    } as any);

    vi.mocked(argon2.verify).mockResolvedValue(true);

    const identity = await provider.authenticate(req);
    expect(identity).not.toBeNull();
    expect(identity?.sub).toBe('user-123');
    expect(identity?.roles).toContain('operator');
    expect(db.query).toHaveBeenCalledWith(expect.stringContaining('UPDATE api_keys'), ['key-id']);
  });

  it('should throw if database key is invalid', async () => {
    const key = 'sera_invalid_key';
    const req = { headers: { authorization: `Bearer ${key}` } } as any;

    vi.mocked(db.query).mockResolvedValueOnce({
      rowCount: 1,
      rows: [{
        id: 'key-id',
        key_hash: 'different_hash',
        owner_sub: 'user-123',
        roles: ['operator']
      }]
    } as any);

    vi.mocked(argon2.verify).mockResolvedValue(false);

    await expect(provider.authenticate(req)).rejects.toThrow('Invalid API key');
  });

  it('should return null if key does not start with sera_', async () => {
    const key = 'not_sera_prefix';
    const req = { headers: { authorization: `Bearer ${key}` } } as any;

    const identity = await provider.authenticate(req);
    expect(identity).toBeNull();
  });
});
