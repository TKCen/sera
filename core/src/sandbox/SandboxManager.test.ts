import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SandboxManager } from './SandboxManager.js';
import { PolicyViolationError } from './TierPolicy.js';
import type { AgentManifest } from '../agents/manifest/types.js';
import type { SpawnRequest } from './types.js';

// ── Mock Docker ─────────────────────────────────────────────────────────────────

function createMockDocker() {
  const mockStream = {
    on: vi.fn((event: string, cb: (data?: any) => void) => {
      if (event === 'end') cb();
      return mockStream;
    }),
  };

  const mockExecInstance = {
    start: vi.fn().mockResolvedValue(mockStream),
    inspect: vi.fn().mockResolvedValue({ ExitCode: 0 }),
  };

  const mockContainer = {
    start: vi.fn().mockResolvedValue(undefined),
    stop: vi.fn().mockResolvedValue(undefined),
    remove: vi.fn().mockResolvedValue(undefined),
    inspect: vi.fn().mockResolvedValue({ Id: 'container-abc123' }),
    exec: vi.fn().mockResolvedValue(mockExecInstance),
    logs: vi.fn().mockResolvedValue(Buffer.from('test log output')),
  };

  return {
    createContainer: vi.fn().mockResolvedValue(mockContainer),
    getContainer: vi.fn().mockReturnValue(mockContainer),
    _container: mockContainer,
    _execInstance: mockExecInstance,
  };
}

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
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('SandboxManager', () => {
  let mockDocker: ReturnType<typeof createMockDocker>;
  let manager: SandboxManager;

  beforeEach(() => {
    mockDocker = createMockDocker();
    manager = new SandboxManager(mockDocker as any);
  });

  describe('spawn', () => {
    it('should create and start a container with tier limits', async () => {
      const manifest = makeManifest();
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine:latest',
        command: ['echo', 'hello'],
      };

      const info = await manager.spawn(manifest, request);

      expect(info.containerId).toBe('container-abc123');
      expect(info.agentName).toBe('test-agent');
      expect(info.type).toBe('tool');
      expect(info.status).toBe('running');
      expect(info.tier).toBe(2);

      // Verify Docker was called with tier 2 limits
      expect(mockDocker.createContainer).toHaveBeenCalledOnce();
      const createArgs = mockDocker.createContainer.mock.calls[0]![0];
      expect(createArgs.HostConfig.CpuShares).toBe(512);
      expect(createArgs.HostConfig.Memory).toBe(512 * 1024 * 1024);
      expect(createArgs.HostConfig.NetworkMode).toBe('sera_net');
      expect(createArgs.Labels['sera.agent']).toBe('test-agent');
    });

    it('should apply tier 1 limits (read-only, no network)', async () => {
      const manifest = makeManifest({
        metadata: {
          name: 'readonly-agent',
          displayName: 'ReadOnly',
          icon: '🔍',
          circle: 'test',
          tier: 1,
        },
      });
      const request: SpawnRequest = {
        agentName: 'readonly-agent',
        type: 'tool',
        image: 'alpine',
      };

      await manager.spawn(manifest, request);

      const createArgs = mockDocker.createContainer.mock.calls[0]![0];
      expect(createArgs.HostConfig.NetworkMode).toBe('none');
      expect(createArgs.HostConfig.CpuShares).toBe(256);
      expect(createArgs.HostConfig.Memory).toBe(256 * 1024 * 1024);
      // Bind mount should be read-only
      expect(createArgs.HostConfig.Binds[0]).toContain(':ro');
    });

    it('should reject subagent spawn when role is not allowed', async () => {
      const manifest = makeManifest({
        subagents: { allowed: [{ role: 'researcher' }] },
      });
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'subagent',
        image: 'node:20',
        subagentRole: 'hacker',
      };

      await expect(manager.spawn(manifest, request))
        .rejects.toThrow(PolicyViolationError);
    });

    it('should enforce maxInstances for subagents', async () => {
      const manifest = makeManifest({
        subagents: { allowed: [{ role: 'researcher', maxInstances: 1 }] },
      });

      // First spawn should succeed
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'subagent',
        image: 'node:20',
        subagentRole: 'researcher',
      };
      await manager.spawn(manifest, request);

      // Second spawn should fail (max 1)
      await expect(manager.spawn(manifest, request))
        .rejects.toThrow(/max instance limit/);
    });
  });

  describe('exec', () => {
    it('should execute a command in a running container', async () => {
      const manifest = makeManifest();
      // First spawn a container
      await manager.spawn(manifest, {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine',
      });

      const result = await manager.exec(manifest, {
        containerId: 'container-abc123',
        agentName: 'test-agent',
        command: ['echo', 'hello'],
      });

      expect(result.exitCode).toBe(0);
      expect(mockDocker._container.exec).toHaveBeenCalledOnce();
    });

    it('should reject exec from a different agent', async () => {
      const manifest = makeManifest();
      await manager.spawn(manifest, {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine',
      });

      const otherManifest = makeManifest({
        metadata: {
          name: 'other-agent',
          displayName: 'Other',
          icon: '🤖',
          circle: 'test',
          tier: 2,
        },
      });

      await expect(
        manager.exec(otherManifest, {
          containerId: 'container-abc123',
          agentName: 'other-agent',
          command: ['cat', '/etc/passwd'],
        }),
      ).rejects.toThrow(PolicyViolationError);
    });

    it('should throw for non-existent container', async () => {
      const manifest = makeManifest();
      await expect(
        manager.exec(manifest, {
          containerId: 'nonexistent',
          agentName: 'test-agent',
          command: ['echo'],
        }),
      ).rejects.toThrow(/not found/);
    });
  });

  describe('remove', () => {
    it('should stop and remove a container', async () => {
      const manifest = makeManifest();
      await manager.spawn(manifest, {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine',
      });

      await manager.remove(manifest, 'container-abc123');

      expect(mockDocker._container.stop).toHaveBeenCalledOnce();
      expect(mockDocker._container.remove).toHaveBeenCalledOnce();
      expect(manager.listContainers()).toHaveLength(0);
    });

    it('should reject removal by non-owner', async () => {
      const manifest = makeManifest();
      await manager.spawn(manifest, {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine',
      });

      const otherManifest = makeManifest({
        metadata: {
          name: 'other-agent',
          displayName: 'Other',
          icon: '🤖',
          circle: 'test',
          tier: 2,
        },
      });

      await expect(manager.remove(otherManifest, 'container-abc123'))
        .rejects.toThrow(PolicyViolationError);
    });
  });

  describe('listContainers', () => {
    it('should list all containers', async () => {
      const manifest = makeManifest();
      await manager.spawn(manifest, {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine',
      });

      const list = manager.listContainers();
      expect(list).toHaveLength(1);
      expect(list[0]!.agentName).toBe('test-agent');
    });

    it('should filter by agent name', async () => {
      const manifest = makeManifest();
      await manager.spawn(manifest, {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine',
      });

      expect(manager.listContainers('test-agent')).toHaveLength(1);
      expect(manager.listContainers('other-agent')).toHaveLength(0);
    });
  });

  describe('getLogs', () => {
    it('should return container logs', async () => {
      const logs = await manager.getLogs('any-container-id');
      expect(logs).toBe('test log output');
    });
  });

  describe('countSubagents', () => {
    it('should count running subagents by role', async () => {
      const manifest = makeManifest({
        subagents: { allowed: [{ role: 'researcher', maxInstances: 5 }] },
      });

      await manager.spawn(manifest, {
        agentName: 'test-agent',
        type: 'subagent',
        image: 'node:20',
        subagentRole: 'researcher',
      });

      expect(manager.countSubagents('test-agent', 'researcher')).toBe(1);
      expect(manager.countSubagents('test-agent', 'browser')).toBe(0);
    });
  });
});
