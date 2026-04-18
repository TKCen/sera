/**
 * ActingContext — formal type for who holds authority, who is acting, and
 * how that authority was acquired. Travels with every tool execution and
 * audit record in Epic 17.
 */

const DELEGATION_MAX_CHAIN_DEPTH = parseInt(process.env['DELEGATION_MAX_CHAIN_DEPTH'] ?? '5', 10);

// ── Core types ───────────────────────────────────────────────────────────────

export interface DelegationScope {
  service: string; // e.g. 'github', 'google-calendar', '*'
  permissions: string[]; // e.g. ['repo:read', 'issues:write'] or ['*']
  resourceConstraints?: Record<string, string[]>; // e.g. { repos: ['org/repo'] }
}

export interface DelegationLink {
  delegatorType: 'operator' | 'agent';
  delegatorId: string; // operatorSub or agentId
  delegatorName: string;
  scope: DelegationScope;
  grantType: 'one-time' | 'session' | 'persistent';
  issuedAt: string; // ISO 8601
  expiresAt?: string;
}

export interface ActingContext {
  principal: {
    type: 'operator' | 'agent';
    id: string; // operatorSub or agentId
    name: string; // email or agentName
    authMethod: 'oidc' | 'api-key' | 'agent-jwt' | 'delegation';
  };
  actor: {
    agentId: string;
    agentName: string;
    instanceId: string;
  };
  delegationChain: DelegationLink[]; // empty = agent acting as own principal
  delegationTokenId?: string; // set when using an active delegation token
}

// ── Builder ──────────────────────────────────────────────────────────────────

export class ActingContextBuilder {
  /**
   * Autonomous context: agent acting as its own principal with its own service
   * identity. Principal === actor; empty delegation chain.
   */
  static buildAutonomous(agentId: string, agentName: string, instanceId: string): ActingContext {
    return {
      principal: {
        type: 'agent',
        id: agentId,
        name: agentName,
        authMethod: 'agent-jwt',
      },
      actor: { agentId, agentName, instanceId },
      delegationChain: [],
    };
  }

  /**
   * Operator-delegated context: agent acts on behalf of an operator.
   * Principal is the operator; actor is the agent.
   */
  static buildDelegated(opts: {
    operatorSub: string;
    operatorName: string;
    operatorAuthMethod: 'oidc' | 'api-key';
    agentId: string;
    agentName: string;
    instanceId: string;
    delegationTokenId: string;
    scope: DelegationScope;
    grantType: 'one-time' | 'session' | 'persistent';
    issuedAt: string;
    expiresAt?: string;
  }): ActingContext {
    const link: DelegationLink = {
      delegatorType: 'operator',
      delegatorId: opts.operatorSub,
      delegatorName: opts.operatorName,
      scope: opts.scope,
      grantType: opts.grantType,
      issuedAt: opts.issuedAt,
      ...(opts.expiresAt !== undefined ? { expiresAt: opts.expiresAt } : {}),
    };
    return {
      principal: {
        type: 'operator',
        id: opts.operatorSub,
        name: opts.operatorName,
        authMethod: opts.operatorAuthMethod,
      },
      actor: { agentId: opts.agentId, agentName: opts.agentName, instanceId: opts.instanceId },
      delegationChain: [link],
      delegationTokenId: opts.delegationTokenId,
    };
  }

  /**
   * Agent-delegated context: parent passes a subset of its authority to a
   * child agent. The delegation chain is extended, not replaced.
   */
  static buildChildDelegated(opts: {
    parentContext: ActingContext;
    childDelegationTokenId: string;
    childAgentId: string;
    childAgentName: string;
    childInstanceId: string;
    narrowedScope: DelegationScope;
    issuedAt: string;
    expiresAt?: string;
  }): ActingContext {
    const link: DelegationLink = {
      delegatorType: 'agent',
      delegatorId: opts.parentContext.actor.agentId,
      delegatorName: opts.parentContext.actor.agentName,
      scope: opts.narrowedScope,
      grantType: 'session',
      issuedAt: opts.issuedAt,
      ...(opts.expiresAt !== undefined ? { expiresAt: opts.expiresAt } : {}),
    };
    return {
      principal: opts.parentContext.principal,
      actor: {
        agentId: opts.childAgentId,
        agentName: opts.childAgentName,
        instanceId: opts.childInstanceId,
      },
      delegationChain: [...opts.parentContext.delegationChain, link],
      delegationTokenId: opts.childDelegationTokenId,
    };
  }

  // ── Validation ─────────────────────────────────────────────────────────────

  /**
   * Validates delegation chain depth and basic integrity.
   */
  static validate(ctx: ActingContext): { valid: boolean; error?: string } {
    if (ctx.delegationChain.length > DELEGATION_MAX_CHAIN_DEPTH) {
      return {
        valid: false,
        error: `Delegation chain depth ${ctx.delegationChain.length} exceeds maximum ${DELEGATION_MAX_CHAIN_DEPTH}`,
      };
    }

    for (let i = 1; i < ctx.delegationChain.length; i++) {
      const curr = ctx.delegationChain[i]!;
      if (curr.delegatorType !== 'agent') {
        return {
          valid: false,
          error: `Link ${i}: only agents can be delegators after the initial operator link`,
        };
      }
    }

    return { valid: true };
  }

  /**
   * Validates that narrowedScope is a strict subset of parentScope.
   * Returns error string if escalation detected, null if valid.
   */
  static validateScopeNarrowing(
    parentScope: DelegationScope,
    narrowedScope: DelegationScope
  ): string | null {
    if (parentScope.service !== '*' && narrowedScope.service !== parentScope.service) {
      return `Cannot narrow scope to service "${narrowedScope.service}" — parent scope is "${parentScope.service}"`;
    }

    if (!parentScope.permissions.includes('*')) {
      for (const perm of narrowedScope.permissions) {
        if (perm !== '*' && !parentScope.permissions.includes(perm)) {
          return `Permission "${perm}" not in parent scope ${JSON.stringify(parentScope.permissions)}`;
        }
      }
      if (narrowedScope.permissions.includes('*')) {
        return `Cannot grant wildcard permissions — parent scope does not include '*'`;
      }
    }

    if (narrowedScope.resourceConstraints && parentScope.resourceConstraints) {
      for (const [key, values] of Object.entries(narrowedScope.resourceConstraints)) {
        const parentValues = parentScope.resourceConstraints[key];
        if (!parentValues) {
          return `Resource constraint key "${key}" not in parent scope`;
        }
        for (const v of values) {
          if (!parentValues.includes(v)) {
            return `Resource constraint value "${v}" for key "${key}" not in parent scope`;
          }
        }
      }
    }

    return null;
  }
}
