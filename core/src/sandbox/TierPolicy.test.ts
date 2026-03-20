import { describe, it, expect } from 'vitest';
import { TierPolicy, PolicyViolationError } from './TierPolicy.js';
import type { AgentManifest } from '../agents/manifest/types.js';
import type { SpawnRequest, ToolRunRequest } from './types.js';

// ── Helpers ─────────────────────────────────────────────────────────────────────

function makeManifest(overrides?: Partial<AgentManifest>): AgentManifest {
  return {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: {
      name: 'test-agent',
      displayName: 'Test Agent',
      icon: '🤖',
      circle: 'test-circle',
      tier: 2,
    },
    identity: {
      role: 'Tester',
      description: 'A test agent',
    },
    model: {
      provider: 'lm-studio',
      name: 'test-model',
    },
    ...overrides,
  } as AgentManifest;
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('TierPolicy', () => {
  describe('getTierLimits boundaries', () => {
    it('should correctly enforce Tier 1 boundaries: no network, ro filesystem', () => {
      const limits = TierPolicy.getTierLimits(1);
      expect(limits.networkMode).toBe('none');
      expect(limits.filesystemMode).toBe('ro');
    });

    it('should correctly enforce Tier 2 boundaries: sera_net network, rw filesystem', () => {
      const limits = TierPolicy.getTierLimits(2);
      expect(limits.networkMode).toBe('agent_net');
      expect(limits.filesystemMode).toBe('rw');
    });

    it('should correctly enforce Tier 3 boundaries: bridge network, rw filesystem', () => {
      const limits = TierPolicy.getTierLimits(3);
      expect(limits.networkMode).toBe('bridge');
      expect(limits.filesystemMode).toBe('rw');
    });
  });

  describe('canExec', () => {
    it('should return true for Tier 2 by default', () => {
      const manifest = makeManifest({
        metadata: { name: 't2', displayName: 'T2', icon: '🤖', circle: 'c', tier: 2 },
      });
      expect(TierPolicy.canExec(manifest)).toBe(true);
    });

    it('should return false for Tier 1 by default', () => {
      const manifest = makeManifest({
        metadata: { name: 't1', displayName: 'T1', icon: '🤖', circle: 'c', tier: 1 },
      });
      expect(TierPolicy.canExec(manifest)).toBe(false);
    });

    it('should honor explicit permission override for Tier 1', () => {
      const manifest = makeManifest({
        metadata: { name: 't1', displayName: 'T1', icon: '🤖', circle: 'c', tier: 1 },
        permissions: { canExec: true },
      });
      expect(TierPolicy.canExec(manifest)).toBe(true);
    });

    it('should honor explicit permission override for Tier 2', () => {
      const manifest = makeManifest({
        metadata: { name: 't2', displayName: 'T2', icon: '🤖', circle: 'c', tier: 2 },
        permissions: { canExec: false },
      });
      expect(TierPolicy.canExec(manifest)).toBe(false);
    });
  });

  describe('canSpawnSubagents', () => {
    it('should return true for Tier 2 by default', () => {
      const manifest = makeManifest({
        metadata: { name: 't2', displayName: 'T2', icon: '🤖', circle: 'c', tier: 2 },
      });
      expect(TierPolicy.canSpawnSubagents(manifest)).toBe(true);
    });

    it('should return false for Tier 1 by default', () => {
      const manifest = makeManifest({
        metadata: { name: 't1', displayName: 'T1', icon: '🤖', circle: 'c', tier: 1 },
      });
      expect(TierPolicy.canSpawnSubagents(manifest)).toBe(false);
    });

    it('should honor explicit permission override for Tier 1', () => {
      const manifest = makeManifest({
        metadata: { name: 't1', displayName: 'T1', icon: '🤖', circle: 'c', tier: 1 },
        permissions: { canSpawnSubagents: true },
      });
      expect(TierPolicy.canSpawnSubagents(manifest)).toBe(true);
    });
  });

  describe('isMemberOfCircle', () => {
    it('should return true for primary circle', () => {
      const manifest = makeManifest({
        metadata: { name: 'a', displayName: 'A', icon: '🤖', circle: 'circle-a', tier: 2 },
      });
      expect(TierPolicy.isMemberOfCircle(manifest, 'circle-a')).toBe(true);
    });

    it('should return true for additional circles', () => {
      const manifest = makeManifest({
        metadata: {
          name: 'a',
          displayName: 'A',
          icon: '🤖',
          circle: 'circle-a',
          additionalCircles: ['circle-b'],
          tier: 2,
        },
      });
      expect(TierPolicy.isMemberOfCircle(manifest, 'circle-b')).toBe(true);
    });

    it('should return false for unknown circles', () => {
      const manifest = makeManifest({
        metadata: { name: 'a', displayName: 'A', icon: '🤖', circle: 'circle-a', tier: 2 },
      });
      expect(TierPolicy.isMemberOfCircle(manifest, 'circle-c')).toBe(false);
    });
  });

  describe('getTierLimits', () => {
    it('should return correct limits for tier 1 (Read Only)', () => {
      const limits = TierPolicy.getTierLimits(1);
      expect(limits.tier).toBe(1);
      expect(limits.networkMode).toBe('none');
      expect(limits.filesystemMode).toBe('ro');
      expect(limits.cpuShares).toBe(256);
      expect(limits.memoryBytes).toBe(256 * 1024 * 1024);
    });

    it('should return correct limits for tier 2 (Internal)', () => {
      const limits = TierPolicy.getTierLimits(2);
      expect(limits.tier).toBe(2);
      expect(limits.networkMode).toBe('agent_net');
      expect(limits.filesystemMode).toBe('rw');
      expect(limits.cpuShares).toBe(512);
      expect(limits.memoryBytes).toBe(512 * 1024 * 1024);
    });

    it('should return correct limits for tier 3 (Executive)', () => {
      const limits = TierPolicy.getTierLimits(3);
      expect(limits.tier).toBe(3);
      expect(limits.networkMode).toBe('bridge');
      expect(limits.filesystemMode).toBe('rw');
      expect(limits.cpuShares).toBe(1024);
      expect(limits.memoryBytes).toBe(1024 * 1024 * 1024);
    });
  });

  describe('getEffectiveLimits', () => {
    it('should use tier defaults when manifest has no resource overrides', () => {
      const manifest = makeManifest();
      const limits = TierPolicy.getEffectiveLimits(manifest);
      expect(limits.cpuShares).toBe(512);
      expect(limits.memoryBytes).toBe(512 * 1024 * 1024);
    });

    it('should apply manifest memory override when within tier ceiling', () => {
      const manifest = makeManifest({ resources: { memory: '256Mi' } });
      const limits = TierPolicy.getEffectiveLimits(manifest);
      expect(limits.memoryBytes).toBe(256 * 1024 * 1024);
    });

    it('should ignore manifest memory override when exceeding tier ceiling', () => {
      const manifest = makeManifest({ resources: { memory: '2Gi' } });
      const limits = TierPolicy.getEffectiveLimits(manifest);
      // Tier 2 max is 512Mi, so 2Gi exceeds and is ignored
      expect(limits.memoryBytes).toBe(512 * 1024 * 1024);
    });

    it('should apply manifest CPU override when within tier ceiling', () => {
      const manifest = makeManifest({ resources: { cpu: '0.25' } });
      const limits = TierPolicy.getEffectiveLimits(manifest);
      // 0.25 * 1024 = 256, tier 2 max is 512
      expect(limits.cpuShares).toBe(256);
    });
  });

  describe('validateSpawnPermission', () => {
    it('should allow subagent spawn when role is in allowed list', () => {
      const manifest = makeManifest({
        subagents: {
          allowed: [{ role: 'researcher', maxInstances: 3 }],
        },
      });
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'subagent',
        image: 'node:20',
        subagentRole: 'researcher',
      };

      expect(() => TierPolicy.validateSpawnPermission(manifest, request)).not.toThrow();
    });

    it('should reject subagent spawn when role is not allowed', () => {
      const manifest = makeManifest({
        subagents: {
          allowed: [{ role: 'researcher', maxInstances: 3 }],
        },
      });
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'subagent',
        image: 'node:20',
        subagentRole: 'hacker',
      };

      expect(() => TierPolicy.validateSpawnPermission(manifest, request)).toThrow(
        PolicyViolationError
      );
    });

    it('should reject subagent spawn when no subagentRole is provided', () => {
      const manifest = makeManifest();
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'subagent',
        image: 'node:20',
      };

      expect(() => TierPolicy.validateSpawnPermission(manifest, request)).toThrow(/subagentRole/);
    });

    it('should allow tool spawn without subagent validation', () => {
      const manifest = makeManifest();
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine',
      };

      expect(() => TierPolicy.validateSpawnPermission(manifest, request)).not.toThrow();
    });

    it('should reject subagent spawn for Tier 1 by default', () => {
      const manifest = makeManifest({
        metadata: { name: 't1', displayName: 'T1', icon: '🤖', circle: 'c', tier: 1 },
      });
      const request: SpawnRequest = {
        agentName: 't1',
        type: 'subagent',
        image: 'node:20',
        subagentRole: 'researcher',
      };
      expect(() => TierPolicy.validateSpawnPermission(manifest, request)).toThrow(
        PolicyViolationError
      );
    });
  });

  describe('validateToolPermission', () => {
    it('should allow tool when in allowed list', () => {
      const manifest = makeManifest({
        tools: { allowed: ['file-read', 'web-search'] },
      });
      const request: ToolRunRequest = {
        agentName: 'test-agent',
        toolName: 'file-read',
        command: ['cat', 'file.txt'],
      };

      expect(() => TierPolicy.validateToolPermission(manifest, request)).not.toThrow();
    });

    it('should reject tool when not in allowed list', () => {
      const manifest = makeManifest({
        tools: { allowed: ['file-read'] },
      });
      const request: ToolRunRequest = {
        agentName: 'test-agent',
        toolName: 'shell-exec',
        command: ['bash'],
      };

      expect(() => TierPolicy.validateToolPermission(manifest, request)).toThrow(
        PolicyViolationError
      );
    });

    it('should reject tool when in denied list', () => {
      const manifest = makeManifest({
        tools: {
          allowed: ['file-read', 'shell-exec'],
          denied: ['shell-exec'],
        },
      });
      const request: ToolRunRequest = {
        agentName: 'test-agent',
        toolName: 'shell-exec',
        command: ['bash'],
      };

      expect(() => TierPolicy.validateToolPermission(manifest, request)).toThrow(
        /explicitly denied/
      );
    });

    it('should allow any tool when allowed list is empty', () => {
      const manifest = makeManifest({ tools: { allowed: [] } });
      const request: ToolRunRequest = {
        agentName: 'test-agent',
        toolName: 'anything',
        command: ['echo'],
      };

      expect(() => TierPolicy.validateToolPermission(manifest, request)).not.toThrow();
    });
  });

  describe('checkInstanceLimit', () => {
    it('should allow spawning when under the limit', () => {
      const manifest = makeManifest({
        subagents: { allowed: [{ role: 'researcher', maxInstances: 3 }] },
      });

      expect(() => TierPolicy.checkInstanceLimit(manifest, 'researcher', 2)).not.toThrow();
    });

    it('should reject spawning when at the limit', () => {
      const manifest = makeManifest({
        subagents: { allowed: [{ role: 'researcher', maxInstances: 3 }] },
      });

      expect(() => TierPolicy.checkInstanceLimit(manifest, 'researcher', 3)).toThrow(
        /max instance limit/
      );
    });

    it('should allow spawning when maxInstances is not set', () => {
      const manifest = makeManifest({
        subagents: { allowed: [{ role: 'researcher' }] },
      });

      expect(() => TierPolicy.checkInstanceLimit(manifest, 'researcher', 100)).not.toThrow();
    });
  });

  describe('clampTimeout', () => {
    it('should return default for undefined', () => {
      expect(TierPolicy.clampTimeout(undefined)).toBe(60);
    });

    it('should return default for 0 or negative', () => {
      expect(TierPolicy.clampTimeout(0)).toBe(60);
      expect(TierPolicy.clampTimeout(-5)).toBe(60);
    });

    it('should clamp to max', () => {
      expect(TierPolicy.clampTimeout(600)).toBe(300);
    });

    it('should pass through valid values', () => {
      expect(TierPolicy.clampTimeout(120)).toBe(120);
    });
  });
});
