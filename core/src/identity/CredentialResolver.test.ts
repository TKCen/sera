import { describe, it, expect, vi, beforeEach } from 'vitest';

// ── Hoist mock variables so they are available inside vi.mock factories ──────

const { mockQuery, mockGet, mockList } = vi.hoisted(() => ({
  mockQuery: vi.fn(),
  mockGet: vi.fn(),
  mockList: vi.fn(),
}));

vi.mock('../lib/database.js', () => ({
  pool: { query: mockQuery },
}));

vi.mock('../secrets/secrets-manager.js', () => ({
  SecretsManager: {
    getInstance: () => ({ get: mockGet, list: mockList }),
  },
}));

import { CredentialResolver } from './CredentialResolver.js';
import type { ActingContext } from './acting-context.js';

// ── Helpers ─────────────────────────────────────────────────────────────────

function makeAutonomousCtx(): ActingContext {
  return {
    principal: { type: 'agent', id: 'agt-1', name: 'Coder', authMethod: 'agent-jwt' },
    actor: { agentId: 'agt-1', agentName: 'Coder', instanceId: 'inst-1' },
    delegationChain: [],
  };
}

function makeDelegatedCtx(tokenId = 'tok-1'): ActingContext {
  return {
    principal: { type: 'operator', id: 'user|op', name: 'alice@example.com', authMethod: 'oidc' },
    actor: { agentId: 'agt-1', agentName: 'Coder', instanceId: 'inst-1' },
    delegationChain: [
      {
        delegatorType: 'operator',
        delegatorId: 'user|op',
        delegatorName: 'alice@example.com',
        scope: { service: 'github', permissions: ['repo:read'] },
        grantType: 'session',
        issuedAt: '2026-01-01T00:00:00Z',
      },
    ],
    delegationTokenId: tokenId,
  };
}

const AGENT_ID = 'agt-1';
const INSTANCE_ID = 'inst-1';

// ── Tests ────────────────────────────────────────────────────────────────────

describe('CredentialResolver', () => {
  let resolver: CredentialResolver;

  beforeEach(() => {
    resolver = new CredentialResolver();
    vi.clearAllMocks();
  });

  describe('Path 1: delegation token', () => {
    it('resolves credential from active delegation token', async () => {
      // DB: delegation_tokens query returns a valid token
      mockQuery.mockResolvedValueOnce({
        rows: [{
          id: 'tok-1',
          actor_agent_id: AGENT_ID,
          scope: { service: 'github', permissions: ['repo:read'] },
          grant_type: 'session',
          credential_secret_name: 'gh-token',
        }],
      });
      // DB: UPDATE use_count
      mockQuery.mockResolvedValueOnce({ rows: [] });
      // Secrets: returns the value
      mockGet.mockResolvedValueOnce('ghp_secret123');

      const result = await resolver.resolve(
        'github',
        AGENT_ID,
        INSTANCE_ID,
        makeDelegatedCtx('tok-1'),
      );

      expect(result).not.toBeNull();
      expect(result!.sourceType).toBe('delegation');
      expect(result!.sourceId).toBe('tok-1');
      expect(result!.value).toBe('ghp_secret123');
    });

    it('revokes one-time grant after use', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{
          id: 'tok-ot',
          actor_agent_id: AGENT_ID,
          scope: { service: 'github', permissions: ['*'] },
          grant_type: 'one-time',
          credential_secret_name: 'gh-token',
        }],
      });
      mockQuery.mockResolvedValueOnce({ rows: [] }); // UPDATE with revoked_at
      mockGet.mockResolvedValueOnce('ghp_one_time');

      const result = await resolver.resolve('github', AGENT_ID, INSTANCE_ID, makeDelegatedCtx('tok-ot'));

      expect(result!.sourceType).toBe('delegation');
      // Verify the UPDATE included revoked_at
      const updateCall = mockQuery.mock.calls[1]!;
      expect((updateCall[0] as string)).toMatch(/revoked_at/);
    });

    it('skips delegation token that does not match service', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{
          id: 'tok-jira',
          actor_agent_id: AGENT_ID,
          scope: { service: 'jira', permissions: ['*'] },
          grant_type: 'session',
          credential_secret_name: 'jira-token',
        }],
      });
      // Falls through to service-identity (empty)
      mockQuery.mockResolvedValueOnce({ rows: [] });
      // Falls through to secret (empty)
      mockList.mockResolvedValueOnce([]);

      const result = await resolver.resolve('github', AGENT_ID, INSTANCE_ID, makeDelegatedCtx('tok-jira'));
      expect(result).toBeNull();
    });

    it('skips revoked delegation token (no rows returned)', async () => {
      // DB returns no rows because revoked_at IS NOT NULL
      mockQuery.mockResolvedValueOnce({ rows: [] });
      // Falls through to service-identity
      mockQuery.mockResolvedValueOnce({ rows: [] });
      // Falls through to secret
      mockList.mockResolvedValueOnce([]);

      const result = await resolver.resolve('github', AGENT_ID, INSTANCE_ID, makeDelegatedCtx('tok-rev'));
      expect(result).toBeNull();
    });
  });

  describe('Path 2: service identity', () => {
    it('resolves credential from service identity when no delegation token', async () => {
      // No delegationTokenId in ctx → skip path 1
      // Service identity query returns a match
      mockQuery.mockResolvedValueOnce({
        rows: [{
          id: 'si-1',
          agent_scope: INSTANCE_ID,
          service: 'github',
          credential_secret_name: 'gh-bot-token',
        }],
      });
      mockGet.mockResolvedValueOnce('ghp_bot_token');

      const result = await resolver.resolve('github', AGENT_ID, INSTANCE_ID, makeAutonomousCtx());

      expect(result!.sourceType).toBe('service-identity');
      expect(result!.sourceId).toBe('si-1');
      expect(result!.value).toBe('ghp_bot_token');
    });
  });

  describe('Path 3: named secret', () => {
    it('resolves credential from secret when no delegation or service identity', async () => {
      // No delegationTokenId → skip path 1
      // Service identity: empty
      mockQuery.mockResolvedValueOnce({ rows: [] });
      // Secret list: returns a matching secret
      mockList.mockResolvedValueOnce([{
        id: 's-1',
        name: 'my-github-secret',
        allowedAgents: ['agt-1'],
        tags: ['github'],
        exposure: 'per-call',
      }]);
      mockGet.mockResolvedValueOnce('ghp_from_secret');

      const result = await resolver.resolve('github', AGENT_ID, INSTANCE_ID, makeAutonomousCtx());

      expect(result!.sourceType).toBe('secret');
      expect(result!.sourceId).toBe('my-github-secret');
      expect(result!.value).toBe('ghp_from_secret');
    });
  });

  describe('Path 4: no credential (deny)', () => {
    it('returns null when all paths fail', async () => {
      // No delegationTokenId → skip path 1
      // Service identity: empty
      mockQuery.mockResolvedValueOnce({ rows: [] });
      // Secret list: empty
      mockList.mockResolvedValueOnce([]);

      const result = await resolver.resolve('github', AGENT_ID, INSTANCE_ID, makeAutonomousCtx());
      expect(result).toBeNull();
    });

    it('returns null when actingContext is null', async () => {
      // Service identity: empty
      mockQuery.mockResolvedValueOnce({ rows: [] });
      // Secret list: empty
      mockList.mockResolvedValueOnce([]);

      const result = await resolver.resolve('github', AGENT_ID, INSTANCE_ID, null);
      expect(result).toBeNull();
    });
  });
});
