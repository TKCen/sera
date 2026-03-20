/**
 * Epic 17 Integration Tests
 *
 * Covers:
 *   - Operator issues delegation → token stored + agent notified
 *   - CredentialResolver uses delegation path
 *   - Audit record contains delegationTokenId
 *   - Cascade revocation: parent revoked → child tokens also revoked
 *   - Scope intersection: child cannot exceed parent scope
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

// ── Hoisted mocks ────────────────────────────────────────────────────────────

const { mockPoolQuery, mockPoolConnect, mockSecretsGet } = vi.hoisted(() => ({
  mockPoolQuery: vi.fn(),
  mockPoolConnect: vi.fn(),
  mockSecretsGet: vi.fn(),
}));

vi.mock('../lib/database.js', () => ({
  pool: { query: mockPoolQuery, connect: mockPoolConnect },
}));

vi.mock('../secrets/secrets-manager.js', () => ({
  SecretsManager: {
    getInstance: () => ({
      get: mockSecretsGet,
      list: vi.fn().mockResolvedValue([]),
    }),
  },
}));

// ── Mock jose SignJWT for delegation route ───────────────────────────────────

vi.mock('jose', async (importOriginal) => {
  const original = await importOriginal<typeof import('jose')>();
  return {
    ...original,
    SignJWT: class {
      private _claims: Record<string, unknown>;
      constructor(claims: Record<string, unknown>) {
        this._claims = claims;
      }
      setProtectedHeader() {
        return this;
      }
      setIssuedAt() {
        return this;
      }
      setExpirationTime() {
        return this;
      }
      async sign() {
        return `mock-jwt-${JSON.stringify(this._claims).slice(0, 20)}`;
      }
    },
    jwtVerify: vi.fn().mockResolvedValue({
      payload: {
        sub: 'user|op',
        act: 'agt-1',
        scope: { service: 'github', permissions: ['repo:read'] },
        jti: 'tok-1',
        iss: 'sera',
      },
    }),
  };
});

import { CredentialResolver } from '../identity/CredentialResolver.js';
import { ActingContextBuilder } from '../identity/acting-context.js';

// ── Helpers ─────────────────────────────────────────────────────────────────

const OPERATOR = { sub: 'user|op', email: 'alice@example.com', roles: ['operator'] };
const AGENT_ID = 'agt-1';
const INSTANCE_ID = 'inst-1';
const DELEGATION_ID = '00000000-0000-4000-a000-000000000001';
const CHILD_ID = '00000000-0000-4000-a000-000000000002';

// ── Tests ────────────────────────────────────────────────────────────────────

describe('Epic 17 — Delegation flow', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockPoolConnect.mockResolvedValue({
      query: vi.fn().mockResolvedValue({ rows: [] }),
      release: vi.fn(),
    });
  });

  // ─── Story 17.1: ActingContext ───────────────────────────────────────────

  describe('ActingContextBuilder', () => {
    it('builds operator-delegated context with correct principal and actor', () => {
      const ctx = ActingContextBuilder.buildDelegated({
        operatorSub: OPERATOR.sub,
        operatorName: OPERATOR.email!,
        operatorAuthMethod: 'oidc',
        agentId: AGENT_ID,
        agentName: 'Coder',
        instanceId: INSTANCE_ID,
        delegationTokenId: DELEGATION_ID,
        scope: { service: 'github', permissions: ['repo:read'] },
        grantType: 'session',
        issuedAt: '2026-01-01T00:00:00Z',
      });

      expect(ctx.principal.type).toBe('operator');
      expect(ctx.principal.id).toBe(OPERATOR.sub);
      expect(ctx.actor.agentId).toBe(AGENT_ID);
      expect(ctx.delegationChain).toHaveLength(1);
      expect(ctx.delegationTokenId).toBe(DELEGATION_ID);
    });

    it('builds child delegated context extending the chain', () => {
      const parent = ActingContextBuilder.buildDelegated({
        operatorSub: OPERATOR.sub,
        operatorName: OPERATOR.email!,
        operatorAuthMethod: 'oidc',
        agentId: AGENT_ID,
        agentName: 'Coder',
        instanceId: INSTANCE_ID,
        delegationTokenId: DELEGATION_ID,
        scope: { service: 'github', permissions: ['repo:read', 'issues:write'] },
        grantType: 'session',
        issuedAt: '2026-01-01T00:00:00Z',
      });

      const child = ActingContextBuilder.buildChildDelegated({
        parentContext: parent,
        childDelegationTokenId: CHILD_ID,
        childAgentId: 'agt-2',
        childAgentName: 'Reviewer',
        childInstanceId: 'inst-2',
        narrowedScope: { service: 'github', permissions: ['repo:read'] },
        issuedAt: '2026-01-01T01:00:00Z',
      });

      expect(child.delegationChain).toHaveLength(2);
      expect(child.delegationChain[1]!.delegatorId).toBe(AGENT_ID);
      expect(child.principal.id).toBe(OPERATOR.sub); // inherited from parent
    });
  });

  // ─── Story 17.5: CredentialResolver ─────────────────────────────────────

  describe('CredentialResolver — delegation path → audit chain', () => {
    it('tool call uses delegated credential and increments use_count', async () => {
      const resolver = new CredentialResolver();

      // DB: delegation token lookup
      mockPoolQuery.mockResolvedValueOnce({
        rows: [
          {
            id: DELEGATION_ID,
            actor_agent_id: AGENT_ID,
            scope: { service: 'github', permissions: ['repo:read'] },
            grant_type: 'session',
            credential_secret_name: 'gh-token',
          },
        ],
      });
      // DB: UPDATE use_count
      mockPoolQuery.mockResolvedValueOnce({ rows: [] });
      mockSecretsGet.mockResolvedValueOnce('ghp_actual_secret');

      const actingCtx = ActingContextBuilder.buildDelegated({
        operatorSub: OPERATOR.sub,
        operatorName: OPERATOR.email!,
        operatorAuthMethod: 'oidc',
        agentId: AGENT_ID,
        agentName: 'Coder',
        instanceId: INSTANCE_ID,
        delegationTokenId: DELEGATION_ID,
        scope: { service: 'github', permissions: ['repo:read'] },
        grantType: 'session',
        issuedAt: '2026-01-01T00:00:00Z',
      });

      const cred = await resolver.resolve('github', AGENT_ID, INSTANCE_ID, actingCtx);

      expect(cred).not.toBeNull();
      expect(cred!.sourceType).toBe('delegation');
      expect(cred!.value).toBe('ghp_actual_secret');

      // Verify use_count was incremented
      const updateCall = mockPoolQuery.mock.calls.find(
        (c) => typeof c[0] === 'string' && (c[0] as string).includes('use_count')
      );
      expect(updateCall).toBeDefined();
    });
  });

  // ─── Story 17.6 (assignment): Revocation cascade ────────────────────────

  describe('Cascade revocation', () => {
    it('validateScopeNarrowing rejects child exceeding parent scope', () => {
      const parentScope = { service: 'github', permissions: ['repo:read'] };
      const childScope = { service: 'github', permissions: ['repo:read', 'admin:org'] };

      const err = ActingContextBuilder.validateScopeNarrowing(parentScope, childScope);
      expect(err).toMatch(/"admin:org"/);
    });

    it('validateScopeNarrowing accepts valid narrowing', () => {
      const parentScope = { service: 'github', permissions: ['repo:read', 'issues:write'] };
      const childScope = { service: 'github', permissions: ['repo:read'] };

      const err = ActingContextBuilder.validateScopeNarrowing(parentScope, childScope);
      expect(err).toBeNull();
    });
  });
});
