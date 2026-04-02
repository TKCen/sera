import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ContainerSecurityMapper } from './ContainerSecurityMapper.js';
import type { AgentManifest, ResolvedCapabilities } from '../agents/manifest/types.js';
import type { SpawnRequest } from './types.js';

describe('ContainerSecurityMapper', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    vi.resetModules();
    process.env = { ...originalEnv };
  });

  afterEach(() => {
    process.env = originalEnv;
  });

  const baseManifest: AgentManifest = {
    apiVersion: 'v1',
    kind: 'Agent',
    metadata: {
      name: 'test-agent',
      displayName: 'Test Agent',
      icon: '🤖',
      tier: 1,
    },
    identity: { role: 'tester', description: 'testing' },
    model: { provider: 'openai', name: 'gpt-4o' },
  };

  const baseRequest: SpawnRequest = {
    agentName: 'test-agent',
    type: 'agent',
    image: 'sera-agent-worker:latest',
  };

  const baseCaps: ResolvedCapabilities = {};

  it('should map basic agent options correctly', () => {
    const result = ContainerSecurityMapper.mapSecurityOptions(
      baseManifest,
      baseRequest,
      baseCaps,
      'inst-1',
      'test-agent',
      1,
      ['FOO=bar'],
      ['/host:/container'],
      'sera-agent-test-agent-inst-1',
      false
    );

    expect(result.networkMode).toBe('agent_net');
    expect(result.proxyEnabled).toBe(false);
    expect(result.createOptions.name).toBe('sera-agent-test-agent-inst-1');
    expect(result.createOptions.HostConfig?.NetworkMode).toBe('agent_net');
    expect(result.createOptions.HostConfig?.CapDrop).toEqual(['ALL']);
  });

  it('should set networkMode to none for tool type with no outbound caps', () => {
    const toolRequest: SpawnRequest = { ...baseRequest, type: 'tool' };
    const result = ContainerSecurityMapper.mapSecurityOptions(
      baseManifest,
      toolRequest,
      baseCaps,
      'inst-1',
      'test-agent',
      1,
      [],
      [],
      'tool-container',
      true
    );

    expect(result.networkMode).toBe('none');
    expect(result.createOptions.HostConfig?.NetworkMode).toBe('none');
  });

  it('should set networkMode to agent_net for tool type with outbound caps', () => {
    const toolRequest: SpawnRequest = { ...baseRequest, type: 'tool' };
    const capsWithNetwork: ResolvedCapabilities = {
      network: { outbound: ['*.google.com'] },
    };
    const result = ContainerSecurityMapper.mapSecurityOptions(
      baseManifest,
      toolRequest,
      capsWithNetwork,
      'inst-1',
      'test-agent',
      1,
      [],
      [],
      'tool-container',
      true
    );

    expect(result.networkMode).toBe('agent_net');
  });

  it('should enable egress proxy when EGRESS_PROXY_URL is set and network is agent_net', () => {
    process.env.EGRESS_PROXY_URL = 'http://proxy:8080';
    const env: string[] = [];
    const result = ContainerSecurityMapper.mapSecurityOptions(
      baseManifest,
      baseRequest,
      baseCaps,
      'inst-1',
      'test-agent',
      1,
      env,
      [],
      'agent-container',
      false
    );

    expect(result.proxyEnabled).toBe(true);
    expect(env).toContain('HTTP_PROXY=http://proxy:8080');
    expect(env).toContain('HTTPS_PROXY=http://proxy:8080');
    expect(env).toContain('NO_PROXY=sera-core,centrifugo,localhost,127.0.0.1');
  });

  it('should apply resource limits from capabilities', () => {
    const capsWithResources: ResolvedCapabilities = {
      resources: {
        cpu_shares: 512,
        memory_limit: 1024, // 1024 MB
      },
    };
    const result = ContainerSecurityMapper.mapSecurityOptions(
      baseManifest,
      baseRequest,
      capsWithResources,
      'inst-1',
      'test-agent',
      1,
      [],
      [],
      'agent-container',
      false
    );

    expect(result.createOptions.HostConfig?.CpuShares).toBe(512);
    expect(result.createOptions.HostConfig?.Memory).toBe(1024 * 1024 * 1024);
  });

  it('should add CHOWN capability if requested', () => {
    const capsWithChown: ResolvedCapabilities = {
      capabilities: ['CHOWN'],
    };
    const result = ContainerSecurityMapper.mapSecurityOptions(
      baseManifest,
      baseRequest,
      capsWithChown,
      'inst-1',
      'test-agent',
      1,
      [],
      [],
      'agent-container',
      false
    );

    expect(result.createOptions.HostConfig?.CapAdd).toContain('CHOWN');
  });

  it('should set readonly rootfs if configured', () => {
    const capsReadonly: ResolvedCapabilities = {
      security: { readonlyRootfs: true },
    };
    const result = ContainerSecurityMapper.mapSecurityOptions(
      baseManifest,
      baseRequest,
      capsReadonly,
      'inst-1',
      'test-agent',
      1,
      [],
      [],
      'agent-container',
      false
    );

    expect(result.createOptions.HostConfig?.ReadonlyRootfs).toBe(true);
  });

  it('should set correct labels for mcp-server type', () => {
    const mcpRequest: SpawnRequest = { ...baseRequest, type: 'mcp-server' };
    const result = ContainerSecurityMapper.mapSecurityOptions(
      baseManifest,
      mcpRequest,
      baseCaps,
      'inst-1',
      'test-agent',
      1,
      [],
      [],
      'mcp-container',
      false
    );

    expect(result.createOptions.Labels?.['sera.type']).toBe('mcp-server');
    expect(result.createOptions.Labels?.['sera.mcp-server']).toBe('test-agent');
    expect(result.createOptions.Labels?.['sera.agent']).toBeUndefined();
  });
});
