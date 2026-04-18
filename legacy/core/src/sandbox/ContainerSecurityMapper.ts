import type Docker from 'dockerode';
import type { AgentManifest, ResolvedCapabilities } from '../agents/manifest/types.js';
import type { SpawnRequest } from './types.js';

export class ContainerSecurityMapper {
  static mapSecurityOptions(
    manifest: AgentManifest,
    request: SpawnRequest,
    caps: ResolvedCapabilities,
    finalInstanceId: string,
    agentName: string,
    tier: number,
    env: string[],
    binds: string[],
    containerName: string,
    isEphemeral: boolean
  ): {
    createOptions: Docker.ContainerCreateOptions;
    networkMode: string;
    proxyEnabled: boolean;
  } {
    // ── Resource Limits ─────────────────────────────────────────────────────
    // Tier-based defaults when capabilities don't specify explicit resource limits.
    // These prevent unconstrained containers from starving the host.
    const TIER_DEFAULTS: Record<number, { cpuShares: number; memoryMb: number }> = {
      1: { cpuShares: 256, memoryMb: 512 }, // Most restricted
      2: { cpuShares: 512, memoryMb: 1024 }, // Standard
      3: { cpuShares: 1024, memoryMb: 2048 }, // Privileged
    };
    const tierDefaults = TIER_DEFAULTS[tier] ?? TIER_DEFAULTS[1]!;

    const cpuShares = caps.resources?.cpu_shares || tierDefaults.cpuShares;
    const memoryBytes = (caps.resources?.memory_limit || tierDefaults.memoryMb) * 1024 * 1024;

    const TIER_PID_LIMITS: Record<number, number> = {
      1: 64,
      2: 256,
      3: 512,
    };
    const pidsLimit = TIER_PID_LIMITS[tier] ?? 64;

    // ── Network Mode (Story 3.2, 20.3) ──────────────────────────────────────
    // Agent containers always need agent_net to reach sera-core (LLM proxy,
    // task polling, heartbeat) and centrifugo (thought streaming) via the
    // sera_net bridge added post-start. The egress proxy on agent_net is the
    // single exit point for all external traffic.
    // Only non-agent tool containers with no outbound access use 'none'.
    const outbound = caps.network?.outbound || [];
    let networkMode: string;
    if (request.type === 'agent') {
      // Agents always need network for sera-core communication
      networkMode = 'agent_net';
    } else if (outbound.length === 0) {
      networkMode = 'none';
    } else {
      networkMode = 'agent_net';
    }

    // ── Egress Proxy (Story 20.3) ─────────────────────────────────────────
    // Inject proxy env vars so all outbound HTTP traffic routes through the
    // egress proxy. Only active when EGRESS_PROXY_URL is set (graceful
    // degradation if proxy not deployed).
    const egressProxyUrl = process.env.EGRESS_PROXY_URL;
    const proxyEnabled = networkMode === 'agent_net' && !!egressProxyUrl && tier > 1;
    if (proxyEnabled) {
      env.push(`HTTP_PROXY=${egressProxyUrl}`);
      env.push(`HTTPS_PROXY=${egressProxyUrl}`);
      env.push('NO_PROXY=sera-core,centrifugo,localhost,127.0.0.1');
    }

    // ── Linux Capabilities (Story 3.2) ──────────────────────────────────────
    const linuxCaps: string[] = Array.isArray(caps.capabilities) ? caps.capabilities : [];

    const createOptions: Docker.ContainerCreateOptions = {
      name: containerName,
      Image: manifest.spec?.sandbox?.image ?? request.image ?? 'sera-agent-worker:latest',
      Cmd: manifest.spec?.sandbox?.command ?? request.command ?? undefined,
      ...(manifest.spec?.sandbox?.entrypoint
        ? { Entrypoint: manifest.spec.sandbox.entrypoint }
        : {}),
      Env: env,
      ExposedPorts: { [`${manifest.spec?.sandbox?.chatPort ?? 3100}/tcp`]: {} },
      AttachStdin: !!request.task,
      OpenStdin: !!request.task,
      StdinOnce: !!request.task,
      WorkingDir: request.type === 'mcp-server' ? undefined : '/workspace',
      Labels: {
        'sera.sandbox': 'true',
        ...(request.type === 'mcp-server'
          ? { 'sera.mcp-server': agentName }
          : { 'sera.agent': agentName }),
        'sera.instance': finalInstanceId,
        'sera.type': request.type,
        'sera.tier': String(tier),
        'sera.circle': manifest.metadata.circle ?? 'default',
      },
      HostConfig: {
        CpuShares: cpuShares,
        Memory: memoryBytes,
        PidsLimit: pidsLimit,
        NetworkMode: networkMode,
        Binds: binds,
        AutoRemove: isEphemeral,
        CapDrop: ['ALL'],
        ...(linuxCaps.length > 0 ? { CapAdd: linuxCaps } : {}),
        ReadonlyRootfs: caps.security?.readonlyRootfs ?? false,
        ...(tier === 1 ? { SecurityOpt: ['no-new-privileges'] } : {}),
      },
    };

    if (caps.capabilities?.includes('CHOWN')) {
      createOptions.HostConfig!.CapAdd = createOptions.HostConfig!.CapAdd || [];
      createOptions.HostConfig!.CapAdd.push('CHOWN');
    }

    return { createOptions, networkMode, proxyEnabled };
  }
}
