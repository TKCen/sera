import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SandboxManager } from './SandboxManager.js';
import type { AgentManifest } from '../agents/index.js';
import type { SpawnRequest } from './types.js';

vi.mock('fs');
vi.mock('path', async () => {
  const actual = (await vi.importActual('path')) as Record<string, unknown>;
  return {
    ...actual,
    join: vi.fn((...args: string[]) => args.join('/')), // Simplified for tests
    resolve: vi.fn((...args: string[]) => args.join('/')),
    dirname: vi.fn((p: string) => p.split('/').slice(0, -1).join('/')),
  };
});

// ── Mock Docker ─────────────────────────────────────────────────────────────────

function createMockDocker() {
  const mockStream = {
    on: vi.fn((event: string, cb: (data?: unknown) => void) => {
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
  } as unknown as AgentManifest;
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('SandboxManager', () => {
  let mockDocker: ReturnType<typeof createMockDocker>;
  let manager: SandboxManager;

  beforeEach(() => {
    mockDocker = createMockDocker();
    manager = new SandboxManager(mockDocker as unknown as import('dockerode'));
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

      const resolved = { resources: { cpu_shares: 512, memory_limit: 512 } };
      const info = await manager.spawn(manifest, request, resolved, 'inst-123');

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

      const resolved = {
        resources: { cpu_shares: 256, memory_limit: 256 },
        fs: { write: false },
      };
      await manager.spawn(manifest, request, resolved, 'inst-ro');

      const createArgs = mockDocker.createContainer.mock.calls[0]![0];
      expect(createArgs.HostConfig.NetworkMode).toBe('none');
      expect(createArgs.HostConfig.CpuShares).toBe(256);
      expect(createArgs.HostConfig.Memory).toBe(256 * 1024 * 1024);
      // Bind mount should be read-only for workspace
      const binds = createArgs.HostConfig.Binds;
      const workspaceBind = binds?.find((b: string) =>
        b.replace(/\\/g, '/').includes('/workspaces/inst-ro:/workspace:ro')
      );
      expect(workspaceBind).toBeDefined();
      expect(workspaceBind).toContain(':ro');
    });

    it('should use agent_net for wildcard outbound (Story 20.3)', async () => {
      const manifest = makeManifest();
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'agent',
        image: 'sera-agent-worker:latest',
      };

      const resolved = {
        network: { outbound: ['*'] },
      };
      await manager.spawn(manifest, request, resolved, 'inst-wildcard');

      const createArgs = mockDocker.createContainer.mock.calls[0]![0];
      // All outbound agents use agent_net (no more bridge mode)
      expect(createArgs.HostConfig.NetworkMode).toBe('agent_net');
    });

    it('should inject proxy env vars when EGRESS_PROXY_URL is set (Story 20.3)', async () => {
      process.env.EGRESS_PROXY_URL = 'http://sera-egress-proxy:3128';
      try {
        const manifest = makeManifest();
        const request: SpawnRequest = {
          agentName: 'test-agent',
          type: 'agent',
          image: 'sera-agent-worker:latest',
        };

        const resolved = {
          network: { outbound: ['github.com'] },
        };
        await manager.spawn(manifest, request, resolved, 'inst-proxy');

        const createArgs = mockDocker.createContainer.mock.calls[0]![0];
        const env: string[] = createArgs.Env;
        expect(env).toContain('HTTP_PROXY=http://sera-egress-proxy:3128');
        expect(env).toContain('HTTPS_PROXY=http://sera-egress-proxy:3128');
        expect(env).toContain('NO_PROXY=sera-core,centrifugo,localhost,127.0.0.1');
      } finally {
        delete process.env.EGRESS_PROXY_URL;
      }
    });

    it('should not inject proxy env vars when EGRESS_PROXY_URL is not set', async () => {
      delete process.env.EGRESS_PROXY_URL;
      const manifest = makeManifest();
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'agent',
        image: 'sera-agent-worker:latest',
      };

      const resolved = {
        network: { outbound: ['github.com'] },
      };
      await manager.spawn(manifest, request, resolved, 'inst-no-proxy');

      const createArgs = mockDocker.createContainer.mock.calls[0]![0];
      const env: string[] = createArgs.Env;
      expect(env.some((e: string) => e.startsWith('HTTP_PROXY='))).toBe(false);
    });

    it('should not inject proxy env vars for networkMode none', async () => {
      process.env.EGRESS_PROXY_URL = 'http://sera-egress-proxy:3128';
      try {
        const manifest = makeManifest();
        const request: SpawnRequest = {
          agentName: 'test-agent',
          type: 'tool',
          image: 'alpine',
        };

        // Empty outbound = no network
        const resolved = { network: { outbound: [] } };
        await manager.spawn(manifest, request, resolved, 'inst-none');

        const createArgs = mockDocker.createContainer.mock.calls[0]![0];
        expect(createArgs.HostConfig.NetworkMode).toBe('none');
        const env: string[] = createArgs.Env;
        expect(env.some((e: string) => e.startsWith('HTTP_PROXY='))).toBe(false);
      } finally {
        delete process.env.EGRESS_PROXY_URL;
      }
    });

    it('should capture container IP and set proxyEnabled in SandboxInfo', async () => {
      process.env.EGRESS_PROXY_URL = 'http://sera-egress-proxy:3128';
      const fetchSpy = vi
        .spyOn(globalThis, 'fetch')
        .mockResolvedValue(new Response(JSON.stringify({ ready: true }), { status: 200 }));
      try {
        // First inspect call is the stale container check (no State = skip cleanup),
        // second inspect is post-create to get container IP.
        mockDocker._container.inspect
          .mockResolvedValueOnce({ Id: 'stale-check' })
          .mockResolvedValueOnce({
            Id: 'container-net123',
            NetworkSettings: {
              Networks: {
                agent_net: { IPAddress: '172.19.0.5' },
              },
            },
          });

        const manifest = makeManifest();
        const request: SpawnRequest = {
          agentName: 'test-agent',
          type: 'agent',
          image: 'sera-agent-worker:latest',
        };

        const resolved = { network: { outbound: ['api.openai.com'] } };
        const info = await manager.spawn(manifest, request, resolved, 'inst-ip');

        expect(info.proxyEnabled).toBe(true);
        expect(info.containerIp).toBe('172.19.0.5');
      } finally {
        delete process.env.EGRESS_PROXY_URL;
        fetchSpy.mockRestore();
      }
    });
  });

  describe('persistent grant bind mounts (Story 3.10)', () => {
    it('should include persistent filesystem grants as bind mounts', async () => {
      const mockRegistry = {
        getActiveFilesystemGrants: vi.fn().mockResolvedValue([
          { id: 'grant-1', value: '/data/shared', grant_type: 'persistent' },
          { id: 'grant-2', value: '/opt/models', grant_type: 'persistent' },
        ]),
      };
      manager.setAgentRegistry(
        mockRegistry as unknown as import('../agents/index.js').AgentRegistry
      );

      const manifest = makeManifest();
      const req: SpawnRequest = {
        agentName: 'test-agent',
        type: 'agent',
        image: 'sera-agent-worker:latest',
      };

      await manager.spawn(manifest, req, {}, 'inst-grants');

      const createArgs = mockDocker.createContainer.mock.calls[0]![0];
      const binds: string[] = createArgs.HostConfig.Binds;

      expect(binds).toContain('/data/shared:/data/shared:rw');
      expect(binds).toContain('/opt/models:/opt/models:rw');
      expect(mockRegistry.getActiveFilesystemGrants).toHaveBeenCalledWith('inst-grants');
    });

    it('should skip non-persistent grants', async () => {
      const mockRegistry = {
        getActiveFilesystemGrants: vi
          .fn()
          .mockResolvedValue([{ id: 'grant-1', value: '/data/temp', grant_type: 'session' }]),
      };
      manager.setAgentRegistry(
        mockRegistry as unknown as import('../agents/index.js').AgentRegistry
      );

      const manifest = makeManifest();
      const req: SpawnRequest = {
        agentName: 'test-agent',
        type: 'agent',
        image: 'sera-agent-worker:latest',
      };

      await manager.spawn(manifest, req, {}, 'inst-session-only');

      const createArgs = mockDocker.createContainer.mock.calls[0]![0];
      const binds: string[] = createArgs.HostConfig.Binds;

      expect(binds.some((b: string) => b.includes('/data/temp'))).toBe(false);
    });

    it('should handle registry errors gracefully', async () => {
      const mockRegistry = {
        getActiveFilesystemGrants: vi.fn().mockRejectedValue(new Error('DB connection failed')),
      };
      manager.setAgentRegistry(
        mockRegistry as unknown as import('../agents/index.js').AgentRegistry
      );

      const manifest = makeManifest();
      const req: SpawnRequest = {
        agentName: 'test-agent',
        type: 'agent',
        image: 'sera-agent-worker:latest',
      };

      // Should not throw — error is logged and spawn continues
      const info = await manager.spawn(manifest, req, {}, 'inst-err');
      expect(info.status).toBe('running');
    });

    it('should spawn normally when no registry is set', async () => {
      // No setAgentRegistry call — agentRegistry is undefined
      const manifest = makeManifest();
      const req: SpawnRequest = {
        agentName: 'test-agent',
        type: 'agent',
        image: 'sera-agent-worker:latest',
      };

      const info = await manager.spawn(manifest, req, {}, 'inst-no-reg');
      expect(info.status).toBe('running');
    });
  });

  describe('exec', () => {
    it('should execute a command in a running container', async () => {
      const manifest = makeManifest();
      // First spawn a container
      await manager.spawn(
        manifest,
        {
          agentName: 'test-agent',
          type: 'tool',
          image: 'alpine',
        },
        {},
        'container-abc123'
      );

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
      await manager.spawn(
        manifest,
        {
          agentName: 'test-agent',
          type: 'tool',
          image: 'alpine',
        },
        {},
        'container-abc123'
      );

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
        } as unknown as { containerId: string; agentName: string; command: string[] })
      ).rejects.toThrow(/cannot exec/);
    });
  });

  describe('remove', () => {
    it('should stop and remove a container', async () => {
      const manifest = makeManifest();
      await manager.spawn(
        manifest,
        {
          agentName: 'test-agent',
          type: 'tool',
          image: 'alpine',
        },
        {},
        'container-abc123'
      );

      await manager.remove(manifest, 'container-abc123');

      expect(mockDocker._container.stop).toHaveBeenCalledOnce();
      expect(mockDocker._container.remove).toHaveBeenCalledOnce();
      expect(manager.listContainers()).toHaveLength(0);
    });
  });

  describe('listContainers', () => {
    it('should list all containers', async () => {
      const manifest = makeManifest();
      await manager.spawn(
        manifest,
        {
          agentName: 'test-agent',
          type: 'tool',
          image: 'alpine',
        },
        {},
        'inst-1'
      );

      const list = manager.listContainers();
      expect(list).toHaveLength(1);
      expect(list[0]!.agentName).toBe('test-agent');
    });

    it('should filter by agent name', async () => {
      const manifest = makeManifest();
      await manager.spawn(
        manifest,
        {
          agentName: 'test-agent',
          type: 'tool',
          image: 'alpine',
        },
        {},
        'inst-1'
      );

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

  describe('waitForChatReady', () => {
    it('should resolve when health check returns ready on first try', async () => {
      const fetchSpy = vi
        .spyOn(globalThis, 'fetch')
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ ready: true, busy: false }), { status: 200 })
        );

      await manager.waitForChatReady('http://172.19.0.5:3100', 5000);

      expect(fetchSpy).toHaveBeenCalledTimes(1);
      expect(fetchSpy).toHaveBeenCalledWith(
        'http://172.19.0.5:3100/health',
        expect.objectContaining({ signal: expect.any(AbortSignal) })
      );
      fetchSpy.mockRestore();
    });

    it('should retry on connection failure then succeed', async () => {
      const fetchSpy = vi
        .spyOn(globalThis, 'fetch')
        .mockRejectedValueOnce(new Error('ECONNREFUSED'))
        .mockRejectedValueOnce(new Error('ECONNREFUSED'))
        .mockResolvedValueOnce(new Response(JSON.stringify({ ready: true }), { status: 200 }));

      await manager.waitForChatReady('http://172.19.0.5:3100', 10000);

      expect(fetchSpy).toHaveBeenCalledTimes(3);
      fetchSpy.mockRestore();
    });

    it('should throw when timeout is exceeded', async () => {
      const fetchSpy = vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('ECONNREFUSED'));

      await expect(manager.waitForChatReady('http://172.19.0.5:3100', 500)).rejects.toThrow(
        /did not become ready within 500ms/
      );

      fetchSpy.mockRestore();
    });

    it('should retry when health returns ready: false', async () => {
      const fetchSpy = vi
        .spyOn(globalThis, 'fetch')
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ ready: false, busy: true }), { status: 200 })
        )
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ ready: true, busy: false }), { status: 200 })
        );

      await manager.waitForChatReady('http://172.19.0.5:3100', 5000);

      expect(fetchSpy).toHaveBeenCalledTimes(2);
      fetchSpy.mockRestore();
    });
  });

  describe('spawn readiness integration', () => {
    it('should skip readiness check for non-agent containers', async () => {
      const fetchSpy = vi.spyOn(globalThis, 'fetch');
      const manifest = makeManifest();
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'tool',
        image: 'alpine:latest',
      };

      await manager.spawn(manifest, request, {}, 'inst-tool');

      // fetch should not be called for readiness — tool containers skip it
      expect(fetchSpy).not.toHaveBeenCalled();
      fetchSpy.mockRestore();
    });

    it('should wait for readiness when spawning agent containers with chatUrl', async () => {
      // Mock inspect to return an IP (triggers chatUrl creation)
      mockDocker._container.inspect
        .mockResolvedValueOnce({ Id: 'stale-check' })
        .mockResolvedValueOnce({
          Id: 'container-ready',
          NetworkSettings: {
            Networks: { agent_net: { IPAddress: '172.19.0.10' } },
          },
        });

      const fetchSpy = vi
        .spyOn(globalThis, 'fetch')
        .mockResolvedValueOnce(new Response(JSON.stringify({ ready: true }), { status: 200 }));

      const manifest = makeManifest();
      const request: SpawnRequest = {
        agentName: 'test-agent',
        type: 'agent',
        image: 'sera-agent-worker:latest',
      };

      const info = await manager.spawn(
        manifest,
        request,
        { network: { outbound: ['api.openai.com'] } },
        'inst-ready'
      );

      expect(info.chatUrl).toBe('http://172.19.0.10:3100');
      expect(fetchSpy).toHaveBeenCalledWith('http://172.19.0.10:3100/health', expect.any(Object));
      fetchSpy.mockRestore();
    });
  });

  describe('countSubagents', () => {
    it('should count running subagents by role', async () => {
      const manifest = makeManifest({
        subagents: { allowed: [{ role: 'researcher', maxInstances: 5 }] },
      } as unknown as AgentManifest);

      await manager.spawn(
        manifest,
        {
          agentName: 'test-agent',
          type: 'subagent',
          image: 'node:20',
        },
        {},
        'inst-sub'
      );

      // We need to set subagentRole manually in the request for countSubagents to work
      // since our simplified spawn in test doesn't do role validation anymore
      const sandboxInfo = manager.listContainers()[0]!;
      sandboxInfo.subagentRole = 'researcher';
      sandboxInfo.parentAgent = 'test-agent';

      expect(manager.countSubagents('test-agent', 'researcher')).toBe(1);
      expect(manager.countSubagents('test-agent', 'browser')).toBe(0);
    });
  });
});
