/**
 * TierPolicy — security tier enforcement for sandbox operations.
 *
 * Maps AGENT.yaml security tiers to resource limits, network modes,
 * and filesystem access levels. All sandbox operations go through this
 * policy before reaching Docker.
 *
 * Tier 1 (Read Only)  — No network, read-only workspace
 * Tier 2 (Internal)   — sera_net only, read-write workspace
 * Tier 3 (Executive)  — Full internet, read-write + scratch volume
 *
 * @see sera/docs/reimplementation/agent-workspace-architecture.md § Security Tiers
 */

import type { SecurityTier, AgentManifest } from '../agents/manifest/types.js';
import type { TierLimits, SpawnRequest, ToolRunRequest } from './types.js';

// ── Tier Limit Definitions ──────────────────────────────────────────────────────

const TIER_LIMITS: Record<SecurityTier, TierLimits> = {
  1: {
    tier: 1,
    cpuShares: 256,                    // Low priority
    memoryBytes: 256 * 1024 * 1024,    // 256 Mi
    networkMode: 'none',
    filesystemMode: 'ro',
  },
  2: {
    tier: 2,
    cpuShares: 512,                    // Medium priority
    memoryBytes: 512 * 1024 * 1024,    // 512 Mi
    networkMode: 'sera_net',
    filesystemMode: 'rw',
  },
  3: {
    tier: 3,
    cpuShares: 1024,                   // High priority
    memoryBytes: 1024 * 1024 * 1024,   // 1 Gi
    networkMode: 'bridge',
    filesystemMode: 'rw',
  },
};

// ── Policy Error ────────────────────────────────────────────────────────────────

export class PolicyViolationError extends Error {
  constructor(
    message: string,
    public readonly agentName: string,
    public readonly violation: string,
  ) {
    super(message);
    this.name = 'PolicyViolationError';
  }
}

// ── TierPolicy ──────────────────────────────────────────────────────────────────

export class TierPolicy {
  /**
   * Get the resource limits for a given security tier.
   */
  static getTierLimits(tier: SecurityTier): TierLimits {
    return TIER_LIMITS[tier];
  }

  /**
   * Check if an agent is allowed to execute commands.
   * Prioritizes explicit permissions over tier defaults.
   */
  static canExec(manifest: AgentManifest): boolean {
    if (manifest.permissions?.canExec !== undefined) {
      return manifest.permissions.canExec;
    }
    // Default: Tier 1 cannot exec
    return manifest.metadata.tier !== 1;
  }

  /**
   * Check if an agent is allowed to spawn subagents.
   * Prioritizes explicit permissions over tier defaults.
   */
  static canSpawnSubagents(manifest: AgentManifest): boolean {
    if (manifest.permissions?.canSpawnSubagents !== undefined) {
      return manifest.permissions.canSpawnSubagents;
    }
    // Default: Tier 1 cannot spawn subagents
    return manifest.metadata.tier !== 1;
  }

  /**
   * Check if an agent is a member of a given circle.
   */
  static isMemberOfCircle(manifest: AgentManifest, circleId: string): boolean {
    if (manifest.metadata.circle === circleId) return true;
    return manifest.metadata.additionalCircles?.includes(circleId) ?? false;
  }

  /**
   * Apply manifest-level resource overrides (capped by tier maximums).
   * If the manifest specifies resources, they are used as long as they
   * don't exceed the tier ceiling.
   */
  static getEffectiveLimits(manifest: AgentManifest): TierLimits {
    const base = { ...TIER_LIMITS[manifest.metadata.tier] };

    if (manifest.resources?.memory) {
      const requested = TierPolicy.parseMemory(manifest.resources.memory);
      if (requested > 0 && requested <= base.memoryBytes) {
        base.memoryBytes = requested;
      }
    }

    if (manifest.resources?.cpu) {
      const requested = Math.round(parseFloat(manifest.resources.cpu) * 1024);
      if (requested > 0 && requested <= base.cpuShares) {
        base.cpuShares = requested;
      }
    }

    return base;
  }

  /**
   * Validate that a spawn request is permitted by the agent's manifest.
   * Throws PolicyViolationError on failure.
   */
  static validateSpawnPermission(
    manifest: AgentManifest,
    request: SpawnRequest,
  ): void {
    const agentName = manifest.metadata.name;

    // Subagent validation
    if (request.type === 'subagent') {
      if (!TierPolicy.canSpawnSubagents(manifest)) {
        throw new PolicyViolationError(
          `Agent "${agentName}" is not permitted to spawn subagents`,
          agentName,
          'spawn_not_permitted',
        );
      }

      if (!request.subagentRole) {
        throw new PolicyViolationError(
          `Subagent spawn request must specify a subagentRole`,
          agentName,
          'missing_subagent_role',
        );
      }

      const allowedSubagents = manifest.subagents?.allowed ?? [];
      const entry = allowedSubagents.find(s => s.role === request.subagentRole);

      if (!entry) {
        throw new PolicyViolationError(
          `Agent "${agentName}" is not allowed to spawn subagent role "${request.subagentRole}"`,
          agentName,
          'subagent_role_not_allowed',
        );
      }
    }
  }

  /**
   * Validate that a tool run request is permitted by the agent's manifest.
   * Throws PolicyViolationError on failure.
   */
  static validateToolPermission(
    manifest: AgentManifest,
    request: ToolRunRequest,
  ): void {
    const agentName = manifest.metadata.name;

    // Check denied list first
    if (manifest.tools?.denied?.includes(request.toolName)) {
      throw new PolicyViolationError(
        `Agent "${agentName}" is explicitly denied tool "${request.toolName}"`,
        agentName,
        'tool_denied',
      );
    }

    // Check allowed list (if specified, only those tools are permitted)
    if (manifest.tools?.allowed && manifest.tools.allowed.length > 0) {
      if (!manifest.tools.allowed.includes(request.toolName)) {
        throw new PolicyViolationError(
          `Agent "${agentName}" is not allowed tool "${request.toolName}"`,
          agentName,
          'tool_not_allowed',
        );
      }
    }
  }

  /**
   * Check if spawning another subagent would exceed the maxInstances limit.
   */
  static checkInstanceLimit(
    manifest: AgentManifest,
    subagentRole: string,
    currentCount: number,
  ): void {
    const entry = manifest.subagents?.allowed?.find(s => s.role === subagentRole);
    if (!entry) return; // Already validated by validateSpawnPermission

    if (entry.maxInstances !== undefined && currentCount >= entry.maxInstances) {
      throw new PolicyViolationError(
        `Agent "${manifest.metadata.name}" has reached the max instance limit (${entry.maxInstances}) for subagent role "${subagentRole}"`,
        manifest.metadata.name,
        'max_instances_exceeded',
      );
    }
  }

  /**
   * Clamp a timeout value to the allowed range.
   */
  static clampTimeout(requestedSeconds: number | undefined): number {
    const DEFAULT_TIMEOUT = 60;
    const MAX_TIMEOUT = 300;

    if (requestedSeconds === undefined) return DEFAULT_TIMEOUT;
    if (requestedSeconds <= 0) return DEFAULT_TIMEOUT;
    return Math.min(requestedSeconds, MAX_TIMEOUT);
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────

  /**
   * Parse a Kubernetes-style memory string (e.g. "512Mi", "1Gi") to bytes.
   */
  private static parseMemory(memory: string): number {
    const match = memory.match(/^(\d+(?:\.\d+)?)\s*(Mi|Gi|Ki)?$/);
    if (!match) return 0;

    const value = parseFloat(match[1]!);
    const unit = match[2];

    switch (unit) {
      case 'Ki': return Math.round(value * 1024);
      case 'Mi': return Math.round(value * 1024 * 1024);
      case 'Gi': return Math.round(value * 1024 * 1024 * 1024);
      default:   return Math.round(value); // bytes
    }
  }
}
