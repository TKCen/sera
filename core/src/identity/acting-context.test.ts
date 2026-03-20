import { describe, it, expect, beforeEach } from 'vitest';
import { ActingContextBuilder, type ActingContext, type DelegationScope } from './acting-context.js';

const AGENT = {
  agentId: 'agt-1',
  agentName: 'Coder',
  instanceId: 'inst-1',
};

const OPERATOR = {
  operatorSub: 'user|abc',
  operatorName: 'alice@example.com',
  operatorAuthMethod: 'oidc' as const,
};

const SCOPE: DelegationScope = {
  service: 'github',
  permissions: ['repo:read', 'issues:write'],
};

describe('ActingContextBuilder', () => {
  describe('buildAutonomous()', () => {
    it('sets principal === actor, empty chain, agent-jwt authMethod', () => {
      const ctx = ActingContextBuilder.buildAutonomous(AGENT.agentId, AGENT.agentName, AGENT.instanceId);

      expect(ctx.principal.type).toBe('agent');
      expect(ctx.principal.id).toBe(AGENT.agentId);
      expect(ctx.principal.authMethod).toBe('agent-jwt');
      expect(ctx.actor.agentId).toBe(AGENT.agentId);
      expect(ctx.delegationChain).toHaveLength(0);
      expect(ctx.delegationTokenId).toBeUndefined();
    });
  });

  describe('buildDelegated()', () => {
    it('sets operator as principal, agent as actor, one delegation link', () => {
      const ctx = ActingContextBuilder.buildDelegated({
        ...OPERATOR,
        ...AGENT,
        delegationTokenId: 'tok-1',
        scope: SCOPE,
        grantType: 'session',
        issuedAt: '2026-01-01T00:00:00Z',
        expiresAt: '2026-01-02T00:00:00Z',
      });

      expect(ctx.principal.type).toBe('operator');
      expect(ctx.principal.id).toBe(OPERATOR.operatorSub);
      expect(ctx.principal.authMethod).toBe('oidc');
      expect(ctx.actor.agentId).toBe(AGENT.agentId);
      expect(ctx.delegationChain).toHaveLength(1);
      expect(ctx.delegationChain[0]!.delegatorType).toBe('operator');
      expect(ctx.delegationChain[0]!.delegatorId).toBe(OPERATOR.operatorSub);
      expect(ctx.delegationChain[0]!.scope.service).toBe('github');
      expect(ctx.delegationTokenId).toBe('tok-1');
    });

    it('omits expiresAt when not provided', () => {
      const ctx = ActingContextBuilder.buildDelegated({
        ...OPERATOR,
        ...AGENT,
        delegationTokenId: 'tok-2',
        scope: SCOPE,
        grantType: 'persistent',
        issuedAt: '2026-01-01T00:00:00Z',
      });

      expect(ctx.delegationChain[0]!.expiresAt).toBeUndefined();
    });
  });

  describe('buildChildDelegated()', () => {
    it('extends the parent chain with an agent link', () => {
      const parent = ActingContextBuilder.buildDelegated({
        ...OPERATOR,
        ...AGENT,
        delegationTokenId: 'tok-parent',
        scope: SCOPE,
        grantType: 'session',
        issuedAt: '2026-01-01T00:00:00Z',
      });

      const child = ActingContextBuilder.buildChildDelegated({
        parentContext: parent,
        childDelegationTokenId: 'tok-child',
        childAgentId: 'agt-2',
        childAgentName: 'Reviewer',
        childInstanceId: 'inst-2',
        narrowedScope: { service: 'github', permissions: ['repo:read'] },
        issuedAt: '2026-01-01T01:00:00Z',
      });

      expect(child.principal.id).toBe(OPERATOR.operatorSub);  // inherited
      expect(child.actor.agentId).toBe('agt-2');
      expect(child.delegationChain).toHaveLength(2);
      expect(child.delegationChain[1]!.delegatorType).toBe('agent');
      expect(child.delegationChain[1]!.delegatorId).toBe(AGENT.agentId);
      expect(child.delegationTokenId).toBe('tok-child');
    });
  });

  describe('validate()', () => {
    it('accepts valid autonomous context', () => {
      const ctx = ActingContextBuilder.buildAutonomous(AGENT.agentId, AGENT.agentName, AGENT.instanceId);
      expect(ActingContextBuilder.validate(ctx).valid).toBe(true);
    });

    it('accepts valid delegated context within depth limit', () => {
      const ctx = ActingContextBuilder.buildDelegated({
        ...OPERATOR,
        ...AGENT,
        delegationTokenId: 'tok-1',
        scope: SCOPE,
        grantType: 'session',
        issuedAt: '2026-01-01T00:00:00Z',
      });
      expect(ActingContextBuilder.validate(ctx).valid).toBe(true);
    });

    it('rejects chain exceeding DELEGATION_MAX_CHAIN_DEPTH (default 5)', () => {
      const ctx: ActingContext = ActingContextBuilder.buildAutonomous('a', 'A', 'i');
      // Inject 6 links directly
      ctx.delegationChain = Array.from({ length: 6 }, (_, i) => ({
        delegatorType: 'agent' as const,
        delegatorId: `agt-${i}`,
        delegatorName: `Agent ${i}`,
        scope: SCOPE,
        grantType: 'session' as const,
        issuedAt: '2026-01-01T00:00:00Z',
      }));
      const result = ActingContextBuilder.validate(ctx);
      expect(result.valid).toBe(false);
      expect(result.error).toMatch(/exceeds maximum/);
    });

    it('rejects chain where non-first link has operator delegatorType', () => {
      const ctx: ActingContext = ActingContextBuilder.buildAutonomous('a', 'A', 'i');
      ctx.delegationChain = [
        { delegatorType: 'operator', delegatorId: 'op-1', delegatorName: 'op', scope: SCOPE, grantType: 'session', issuedAt: '2026-01-01T00:00:00Z' },
        { delegatorType: 'operator', delegatorId: 'op-2', delegatorName: 'op2', scope: SCOPE, grantType: 'session', issuedAt: '2026-01-01T00:00:00Z' },
      ];
      const result = ActingContextBuilder.validate(ctx);
      expect(result.valid).toBe(false);
      expect(result.error).toMatch(/only agents can be delegators/);
    });
  });

  describe('validateScopeNarrowing()', () => {
    it('allows valid narrowing of permissions', () => {
      const parent: DelegationScope = { service: 'github', permissions: ['repo:read', 'issues:write'] };
      const child: DelegationScope = { service: 'github', permissions: ['repo:read'] };
      expect(ActingContextBuilder.validateScopeNarrowing(parent, child)).toBeNull();
    });

    it('rejects child claiming permission not in parent', () => {
      const parent: DelegationScope = { service: 'github', permissions: ['repo:read'] };
      const child: DelegationScope = { service: 'github', permissions: ['repo:read', 'admin:org'] };
      expect(ActingContextBuilder.validateScopeNarrowing(parent, child)).toMatch(/"admin:org"/);
    });

    it('rejects child claiming wildcard when parent does not have wildcard', () => {
      const parent: DelegationScope = { service: 'github', permissions: ['repo:read'] };
      const child: DelegationScope = { service: 'github', permissions: ['*'] };
      expect(ActingContextBuilder.validateScopeNarrowing(parent, child)).toMatch(/wildcard/);
    });

    it('rejects child changing the service', () => {
      const parent: DelegationScope = { service: 'github', permissions: ['repo:read'] };
      const child: DelegationScope = { service: 'jira', permissions: ['repo:read'] };
      expect(ActingContextBuilder.validateScopeNarrowing(parent, child)).toMatch(/service/);
    });

    it('allows any service when parent service is wildcard', () => {
      const parent: DelegationScope = { service: '*', permissions: ['*'] };
      const child: DelegationScope = { service: 'github', permissions: ['repo:read'] };
      expect(ActingContextBuilder.validateScopeNarrowing(parent, child)).toBeNull();
    });

    it('rejects child expanding resource constraints beyond parent', () => {
      const parent: DelegationScope = {
        service: 'github',
        permissions: ['repo:read'],
        resourceConstraints: { repos: ['org/repo-a'] },
      };
      const child: DelegationScope = {
        service: 'github',
        permissions: ['repo:read'],
        resourceConstraints: { repos: ['org/repo-a', 'org/repo-b'] },
      };
      expect(ActingContextBuilder.validateScopeNarrowing(parent, child)).toMatch(/repo-b/);
    });
  });
});
