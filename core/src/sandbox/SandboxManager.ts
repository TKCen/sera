/**
 * SandboxManager — container lifecycle management for SERA agents.
 *
 * All Docker operations go through this class. No other module interacts
 * with the Docker daemon directly.
 */

import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import Docker from 'dockerode';
import { v4 as uuidv4 } from 'uuid';
import type { AgentManifest, ResolvedCapabilities } from '../agents/manifest/types.js';
import type { SandboxInfo, SpawnRequest, ExecRequest, DockerLifecycleEvent } from './types.js';
import { PolicyViolationError } from './TierPolicy.js';
import { StorageProviderFactory } from '../storage/StorageProvider.js';
import { LocalStorageProvider } from '../storage/LocalStorageProvider.js';
import { Logger } from '../lib/logger.js';
import { PlatformPath } from '../lib/PlatformPath.js';
import type { EgressAclManager } from './EgressAclManager.js';
import type { AgentRegistry } from '../agents/registry.service.js';

const logger = new Logger('SandboxManager');

// ── SandboxManager ──────────────────────────────────────────────────────────────

export class SandboxManager {
  private docker: Docker;
  /** In-memory map: containerId → SandboxInfo */
  private containers: Map<string, SandboxInfo> = new Map();
  /** Reverse map: instanceId → containerId (for fast teardown lookup) */
  private instanceToContainer: Map<string, string> = new Map();
  private storageFactory: StorageProviderFactory;
  /** Optional egress ACL manager — set via setEgressAclManager() after construction */
  private egressAclManager?: EgressAclManager;
  /** Optional agent registry — set via setAgentRegistry() for Story 3.10 persistent grants */
  private agentRegistry?: AgentRegistry;

  constructor(docker?: Docker, storageFactory?: StorageProviderFactory) {
    this.docker =
      docker ??
      new Docker(
        process.platform === 'win32'
          ? { socketPath: '//./pipe/docker_engine' }
          : { socketPath: '/var/run/docker.sock' }
      );

    if (storageFactory) {
      this.storageFactory = storageFactory;
    } else {
      this.storageFactory = new StorageProviderFactory('local');
      this.storageFactory.register(
        new LocalStorageProvider('/workspaces', process.env.HOST_WORKSPACES_DIR)
      );
    }
  }

  /** Wire up the EgressAclManager after construction (avoids circular deps) */
  setEgressAclManager(mgr: EgressAclManager): void {
    this.egressAclManager = mgr;
  }

  /** Wire up the AgentRegistry for persistent grant bind mounts (Story 3.10) */
  setAgentRegistry(registry: AgentRegistry): void {
    this.agentRegistry = registry;
  }

  // ── Container Spawn ─────────────────────────────────────────────────────────

  /**
   * Spawn a new sandbox container for an agent instance.
   * Story 3.1, 3.3
   */
  async spawn(
    manifest: AgentManifest,
    request: SpawnRequest,
    resolvedCapabilities?: unknown,
    instanceId?: string,
    agentEnvSecrets?: Record<string, string>
  ): Promise<SandboxInfo> {
    const agentName = manifest.metadata.name;
    const finalInstanceId = instanceId ?? `${agentName}-${uuidv4().substring(0, 8)}`;
    const caps = (resolvedCapabilities ?? {}) as ResolvedCapabilities;
    const tier = manifest.metadata.tier ?? 1;

    const containerName = `sera-agent-${agentName.toLowerCase()}-${finalInstanceId.substring(0, 8)}`;

    // ── Environment ─────────────────────────────────────────────────────────
    const env: string[] = [
      ...Object.entries(request.env ?? {}).map(([k, v]) => `${k}=${v}`),
      `AGENT_NAME=${agentName}`,
      `AGENT_INSTANCE_ID=${finalInstanceId}`,
      `SERA_CORE_URL=${process.env.SERA_CORE_URL ?? 'http://sera-core:3001'}`,
      `CENTRIFUGO_API_URL=${process.env.CENTRIFUGO_API_URL ?? 'http://centrifugo:8000/api'}`,
      `CENTRIFUGO_API_KEY=${process.env.CENTRIFUGO_API_KEY ?? ''}`,
      `AGENT_HEARTBEAT_INTERVAL_MS=${process.env.AGENT_HEARTBEAT_INTERVAL_MS ?? '30000'}`,
      `AGENT_LIFECYCLE_MODE=${request.lifecycleMode ?? 'persistent'}`,
      `AGENT_CHAT_PORT=3100`,
    ];

    // Story 3.1 — include identity JWT if provided
    if (request.token) {
      env.push(`SERA_IDENTITY_TOKEN=${request.token}`);
    }

    // Story 16.9 — inject agent-env secrets
    if (agentEnvSecrets) {
      for (const [name, value] of Object.entries(agentEnvSecrets)) {
        env.push(`SERA_SECRET_${name.toUpperCase()}=${value}`);
      }
    }

    // ── Bind Mounts ──────────────────────────────────────────────────────────
    const binds: string[] = [];

    // 1. Workspace mount (Story 3.3)
    const providerName = manifest.workspace?.provider ?? 'local';
    const provider = this.storageFactory.getProvider(providerName);
    const workspacePath = request.hostWorkspacePath ?? manifest.workspace?.path;
    const writeAllowed = caps.filesystem?.write ?? caps.fs?.write ?? false;
    const mode = writeAllowed ? 'rw' : 'ro';
    binds.push(provider.getBindMount(finalInstanceId, '/workspace', mode, workspacePath));

    // Write AGENT.yaml to workspace so the agent-runtime can load its manifest.
    // The runtime expects flat format (identity/model/tools at top level), not spec-wrapped.
    if (request.type !== 'mcp-server') {
      const wsInternalPath = provider.getPath(finalInstanceId, workspacePath);
      fs.mkdirSync(wsInternalPath, { recursive: true });
      // Ensure workspace is writable by the non-root agent user (uid 1001) in the container.
      // The bind mount inherits host permissions, so we must chmod the host directory.
      try {
        fs.chmodSync(wsInternalPath, 0o777);
      } catch {
        // Best-effort — may fail on some host filesystems
      }
      const spec = (manifest.spec ?? {}) as Record<string, unknown>;
      // Use manifest.model (which has instance overrides applied by Orchestrator)
      // instead of spec.model (which is the raw template without overrides).
      const flatModel = ((manifest.model as unknown) ?? spec.model ?? {}) as Record<
        string,
        unknown
      >;
      const modelWithDefaults = {
        ...flatModel,
        ...(flatModel.name
          ? {}
          : { name: process.env.DEFAULT_MODEL ?? process.env.LLM_MODEL ?? 'default' }),
      };
      const manifestYaml = yaml.dump({
        apiVersion: manifest.apiVersion ?? 'sera/v1',
        kind: manifest.kind ?? 'Agent',
        metadata: manifest.metadata,
        identity: spec.identity ?? manifest.identity,
        model: modelWithDefaults,
        tools: spec.tools,
        skills: spec.skills,
        intercom: spec.intercom,
        resources: spec.resources,
        workspace: spec.workspace,
        memory: spec.memory,
        capabilities: spec.capabilities,
        sandboxBoundary: spec.sandboxBoundary,
      });
      fs.writeFileSync(path.join(wsInternalPath, 'AGENT.yaml'), manifestYaml, 'utf-8');
      logger.debug(`Wrote AGENT.yaml to workspace for ${containerName}`);
    }

    // 2. Memory mount (Story 3.3)
    const memoryHostDir = process.env.HOST_MEMORY_DIR ?? '/memory';
    const memoryHostPath = PlatformPath.normalizeDockerBindPath(
      `${memoryHostDir}/${finalInstanceId}`
    );
    fs.mkdirSync(memoryHostPath, { recursive: true });
    binds.push(`${memoryHostPath}:/memory:rw`);

    // 3. Knowledge mounts (Story 3.3)
    const knowledgeHostDir = process.env.HOST_KNOWLEDGE_DIR ?? '/knowledge';
    const personalPath = PlatformPath.normalizeDockerBindPath(
      `${knowledgeHostDir}/agents/${agentName}`
    );
    fs.mkdirSync(personalPath, { recursive: true });
    binds.push(`${personalPath}:/knowledge/personal:ro`);

    const sharedPath = PlatformPath.normalizeDockerBindPath(`${knowledgeHostDir}/shared`);
    fs.mkdirSync(sharedPath, { recursive: true });
    binds.push(`${sharedPath}:/knowledge/shared:ro`);

    // 4. MCP Custom Mounts (Story 7.3)
    if (request.type === 'mcp-server' && manifest.mounts) {
      for (const m of manifest.mounts) {
        const mode = m.mode === 'rw' ? 'rw' : 'ro';
        binds.push(`${m.hostPath}:${m.containerPath}:${mode}`);
      }
    }

    // 5. Persistent filesystem grants (Story 3.10)
    if (this.agentRegistry) {
      try {
        const grants = await this.agentRegistry.getActiveFilesystemGrants(finalInstanceId);
        for (const grant of grants) {
          if (grant.grant_type === 'persistent') {
            // Canonicalise to prevent path traversal
            const grantPath = fs.existsSync(grant.value)
              ? fs.realpathSync(grant.value)
              : grant.value;
            // host path = container path = grant value (rw access)
            binds.push(`${grantPath}:${grantPath}:rw`);
            logger.info(
              `Persistent grant bind mount: ${grantPath} for instance ${finalInstanceId}`
            );
          }
        }
      } catch (err: unknown) {
        logger.error('Failed to load persistent filesystem grants:', err);
      }
    }

    // ── Resource Limits ─────────────────────────────────────────────────────
    const cpuShares = caps.resources?.cpu_shares || 0;
    const memoryBytes = (caps.resources?.memory_limit || 0) * 1024 * 1024;

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
    const proxyEnabled = networkMode === 'agent_net' && !!egressProxyUrl;
    if (proxyEnabled) {
      env.push(`HTTP_PROXY=${egressProxyUrl}`);
      env.push(`HTTPS_PROXY=${egressProxyUrl}`);
      env.push('NO_PROXY=sera-core,centrifugo,localhost,127.0.0.1');
    }

    // ── Linux Capabilities (Story 3.2) ──────────────────────────────────────
    const linuxCaps: string[] = Array.isArray(caps.capabilities) ? caps.capabilities : [];

    // ── Ephemeral auto-remove (Story 3.7) ───────────────────────────────────
    const isEphemeral = request.lifecycleMode === 'ephemeral' || request.type === 'tool';

    // Remove any stale container with the same name (e.g. from a previous crashed run).
    // Only act if inspect returns a real container with State info.
    try {
      const existing = this.docker.getContainer(containerName);
      const info = await existing.inspect();
      const state = (info as { State?: { Status?: string; Running?: boolean } }).State;
      if (state) {
        logger.info(
          `Removing stale container ${containerName} (status: ${state.Status ?? 'unknown'})`
        );
        if (state.Running) await existing.stop().catch(() => {});
        await existing.remove({ force: true }).catch(() => {});
      }
    } catch {
      // Container doesn't exist — expected case
    }

    const createOptions: Docker.ContainerCreateOptions = {
      name: containerName,
      Image: request.image ?? 'sera-agent-worker:latest',
      Cmd: request.command ?? undefined,
      Env: env,
      ExposedPorts: { '3100/tcp': {} },
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
        NetworkMode: networkMode,
        Binds: binds,
        AutoRemove: isEphemeral,
        CapDrop: ['ALL'],
        ...(linuxCaps.length > 0 ? { CapAdd: linuxCaps } : {}),
        ReadonlyRootfs: caps.security?.readonlyRootfs ?? false,
      },
    };

    if (caps.capabilities?.includes('CHOWN')) {
      createOptions.HostConfig!.CapAdd = createOptions.HostConfig!.CapAdd || [];
      createOptions.HostConfig!.CapAdd.push('CHOWN');
    }

    this.audit('spawn', agentName, {
      instanceId: finalInstanceId,
      type: request.type,
      image: request.image,
    });

    const container = await this.docker.createContainer(createOptions);

    if (request.task) {
      const stream = await container.attach({
        stream: true,
        stdin: true,
        stdout: false,
        stderr: false,
      });
      await container.start();

      const taskInput = {
        taskId: `scheduled-${Date.now()}`,
        task: request.task,
      };

      stream.write(JSON.stringify(taskInput) + '\n');
      stream.end();
    } else {
      await container.start();
    }

    // Connect agent containers to sera_net so they can reach sera-core and
    // centrifugo for task polling, LLM proxy, thought streaming, and heartbeats.
    // The primary network (agent_net) remains for egress proxy routing.
    if (networkMode === 'agent_net') {
      try {
        const seraNet = this.docker.getNetwork('sera_net');
        await seraNet.connect({ Container: container.id });
      } catch (netErr) {
        logger.warn(`Failed to connect container to sera_net: ${(netErr as Error).message}`);
      }
    }

    const info = await container.inspect();

    // Extract container IP on agent_net for per-agent ACL mapping (Story 20.2)
    const containerIp =
      networkMode === 'agent_net'
        ? info.NetworkSettings?.Networks?.['agent_net']?.IPAddress || undefined
        : undefined;

    // For chatUrl, use sera_net IP (reachable from sera-core) rather than
    // agent_net IP (only reachable from other agents / egress proxy).
    const seraNetIp =
      networkMode === 'agent_net'
        ? info.NetworkSettings?.Networks?.['sera_net']?.IPAddress || undefined
        : undefined;
    const chatIp = seraNetIp || containerIp;

    const sandboxInfo: SandboxInfo = {
      containerId: info.Id,
      agentName,
      type: request.type,
      image: request.image ?? 'sera-agent-worker:latest',
      status: 'running',
      createdAt: new Date().toISOString(),
      tier,
      instanceId: finalInstanceId,
      ...(request.lifecycleMode !== undefined ? { lifecycleMode: request.lifecycleMode } : {}),
      ...(proxyEnabled ? { proxyEnabled } : {}),
      ...(containerIp ? { containerIp } : {}),
      ...(chatIp ? { chatUrl: `http://${chatIp}:3100` } : {}),
    };

    // Wait for the container's chat server to become ready before
    // marking the container as available.  Without this, the first
    // chat request can arrive before the HTTP server inside the
    // container has started listening, causing a connect timeout.
    if (sandboxInfo.chatUrl && request.type === 'agent') {
      const readyTimeout = parseInt(process.env['AGENT_READY_TIMEOUT_MS'] || '90000', 10);
      await this.waitForChatReady(sandboxInfo.chatUrl, readyTimeout);
    }

    this.containers.set(info.Id, sandboxInfo);
    this.instanceToContainer.set(finalInstanceId, info.Id);

    // Story 20.2 — generate per-agent ACL for the egress proxy
    if (this.egressAclManager && containerIp && outbound.length > 0) {
      this.egressAclManager
        .onSpawn(finalInstanceId, containerIp, { outbound })
        .catch((err: unknown) => logger.error('Failed to write egress ACL:', err));
    }

    return sandboxInfo;
  }

  // ── Container Readiness ────────────────────────────────────────────────────────

  /**
   * Poll the container's chat server health endpoint until it reports ready.
   * Uses exponential backoff starting at 100 ms, capped at 2 000 ms.
   * Throws if `timeoutMs` elapses without a successful health check.
   */
  async waitForChatReady(chatUrl: string, timeoutMs: number): Promise<void> {
    const healthUrl = `${chatUrl}/health`;
    const start = Date.now();
    let delay = 100;
    const maxDelay = 2_000;
    const perRequestTimeout = 3_000;

    while (Date.now() - start < timeoutMs) {
      try {
        const res = await fetch(healthUrl, {
          signal: AbortSignal.timeout(perRequestTimeout),
        });
        if (res.ok) {
          const body = (await res.json()) as { ready?: boolean };
          if (body.ready) {
            const elapsed = Date.now() - start;
            logger.info(`Chat server ready at ${chatUrl} (${elapsed}ms)`);
            return;
          }
        }
      } catch {
        // Connection refused / timeout — container still booting
      }

      await new Promise((resolve) => setTimeout(resolve, delay));
      delay = Math.min(delay * 2, maxDelay);
    }

    throw new Error(`Chat server at ${chatUrl} did not become ready within ${timeoutMs}ms`);
  }

  // ── Teardown (Story 3.7) ─────────────────────────────────────────────────────

  /**
   * Stop the container for an agent instance without deleting workspace data.
   * Called by the cleanup background job and the manual cleanup endpoint.
   */
  async teardown(instanceId: string): Promise<void> {
    const containerId = this.instanceToContainer.get(instanceId);
    if (!containerId) {
      // Try by containerId directly if not found by instanceId
      logger.warn(`teardown: no container found for instanceId=${instanceId}`);
      return;
    }
    const sandbox = this.containers.get(containerId);
    if (sandbox) {
      sandbox.status = 'removing';
    }

    const container = this.docker.getContainer(containerId);
    try {
      await container.stop({ t: 5 });
    } catch {
      // May already be stopped
    }
    try {
      await container.remove({ force: false });
    } catch {
      // Auto-removed ephemeral containers will already be gone
    }

    // Story 20.2 — remove per-agent ACL from the egress proxy
    if (this.egressAclManager) {
      this.egressAclManager
        .onTeardown(instanceId)
        .catch((err: unknown) => logger.error('Failed to remove egress ACL:', err));
    }

    this.containers.delete(containerId);
    this.instanceToContainer.delete(instanceId);
    this.audit('teardown', sandbox?.agentName ?? instanceId, { instanceId, containerId });
  }

  // ── Exec ────────────────────────────────────────────────────────────────────

  async exec(
    manifest: AgentManifest,
    request: ExecRequest
  ): Promise<{ exitCode: number; output: string }> {
    const sandbox = this.containers.get(request.containerId);
    if (!sandbox) {
      throw new Error(`Container "${request.containerId}" not found in sandbox registry`);
    }

    if (
      sandbox.agentName !== manifest.metadata.name &&
      sandbox.parentAgent !== manifest.metadata.name
    ) {
      throw new PolicyViolationError(
        `Agent "${manifest.metadata.name}" cannot exec into container owned by "${sandbox.agentName}"`,
        manifest.metadata.name,
        'exec_not_owner'
      );
    }

    this.audit('exec', manifest.metadata.name, {
      containerId: request.containerId,
      command: request.command,
    });

    const container = this.docker.getContainer(request.containerId);
    const exec = await container.exec({
      Cmd: request.command,
      AttachStdout: true,
      AttachStderr: true,
    });

    const stream = await exec.start({ hijack: true, stdin: false });
    const output = await SandboxManager.collectDemuxedStream(this.docker, stream);
    const inspectResult = await exec.inspect();
    return {
      exitCode: inspectResult.ExitCode ?? -1,
      output,
    };
  }

  // ── Remove (admin / owner-controlled) ──────────────────────────────────────

  async remove(manifest: AgentManifest, containerId: string): Promise<void> {
    const sandbox = this.containers.get(containerId);
    if (!sandbox) {
      throw new Error(`Container "${containerId}" not found in sandbox registry`);
    }

    if (
      sandbox.agentName !== manifest.metadata.name &&
      sandbox.parentAgent !== manifest.metadata.name
    ) {
      throw new PolicyViolationError(
        `Agent "${manifest.metadata.name}" cannot remove container owned by "${sandbox.agentName}"`,
        manifest.metadata.name,
        'remove_not_owner'
      );
    }

    this.audit('remove', manifest.metadata.name, { containerId });
    sandbox.status = 'removing';

    const container = this.docker.getContainer(containerId);
    try {
      await container.stop({ t: 5 });
    } catch {
      /* already stopped */
    }
    try {
      await container.remove({ force: true });
    } catch {
      /* auto-removed */
    }

    this.containers.delete(containerId);
    this.instanceToContainer.delete(sandbox.instanceId);
  }

  // ── Logs (Story 3.5) ─────────────────────────────────────────────────────────

  async getLogs(containerId: string, tail?: number): Promise<string> {
    const container = this.docker.getContainer(containerId);
    const logs = await container.logs({
      stdout: true,
      stderr: true,
      tail: tail ?? 100,
    });
    return typeof logs === 'string' ? logs : (logs as Buffer).toString('utf-8');
  }

  // ── Docker Events Listener (Story 3.5) ──────────────────────────────────────

  /**
   * Attach to the Docker events stream filtered to SERA-managed containers.
   * Calls onEvent for each relevant lifecycle event.
   * Should be called once at startup.
   */
  async startEventListener(onEvent: (event: DockerLifecycleEvent) => Promise<void>): Promise<void> {
    try {
      const eventStream = await this.docker.getEvents({
        filters: JSON.stringify({ label: ['sera.sandbox=true'] }),
      });

      eventStream.on('data', (chunk: Buffer) => {
        try {
          const raw = JSON.parse(chunk.toString('utf-8')) as {
            Type: string;
            Action: string;
            id: string;
            Actor: { Attributes: Record<string, string> };
          };

          if (raw.Type !== 'container') return;

          const action = raw.Action as DockerLifecycleEvent['action'];
          if (!['start', 'stop', 'die', 'oom'].includes(action)) return;

          const labels = raw.Actor.Attributes;
          const instanceId = labels['sera.instance'];
          const agentName = labels['sera.agent'];
          if (!instanceId || !agentName) return;

          const exitCodeStr = labels['exitCode'];
          const exitCode = exitCodeStr !== undefined ? parseInt(exitCodeStr, 10) : undefined;

          const ev: DockerLifecycleEvent = {
            action,
            containerId: raw.id,
            instanceId,
            agentName,
            ...(exitCode !== undefined ? { exitCode } : {}),
          };
          onEvent(ev).catch((err: unknown) => logger.error('Error handling Docker event:', err));
        } catch {
          // Non-JSON chunks (heartbeat from Docker daemon) — ignore
        }
      });

      eventStream.on('error', (err: Error) => {
        logger.error('Docker events stream error:', err);
      });

      logger.info('Docker lifecycle event listener started');
    } catch (err: unknown) {
      logger.warn('Failed to start Docker event listener (is Docker running?):', err);
    }
  }

  // ── Health ───────────────────────────────────────────────────────────────────

  /** Ping the Docker daemon to verify connectivity. */
  async ping(): Promise<void> {
    await this.docker.ping();
  }

  // ── Query ────────────────────────────────────────────────────────────────────

  listContainers(agentName?: string): SandboxInfo[] {
    const all = Array.from(this.containers.values());
    if (!agentName) return all;
    return all.filter((c) => c.agentName === agentName || c.parentAgent === agentName);
  }

  getContainerByInstance(instanceId: string): SandboxInfo | undefined {
    const containerId = this.instanceToContainer.get(instanceId);
    return containerId ? this.containers.get(containerId) : undefined;
  }

  countSubagents(parentAgent: string, role: string): number {
    return Array.from(this.containers.values()).filter(
      (c) => c.parentAgent === parentAgent && c.subagentRole === role && c.status === 'running'
    ).length;
  }

  // ── Dangling Container Check (Story 3.5) ─────────────────────────────────────

  /**
   * Log a warning for any running containers labelled sera.sandbox=true
   * that have no corresponding DB record.
   * @param knownInstanceIds Set of instance IDs currently in the DB.
   */
  async checkDanglingContainers(knownInstanceIds: Set<string>): Promise<void> {
    try {
      const containers = await this.docker.listContainers({
        filters: JSON.stringify({ label: ['sera.sandbox=true'] }),
      });
      for (const c of containers) {
        const instanceId = c.Labels['sera.instance'];
        if (instanceId && !knownInstanceIds.has(instanceId)) {
          logger.warn(
            `Dangling container detected: ${c.Id} (instance=${instanceId}, agent=${c.Labels['sera.agent']})`
          );
        }
      }
    } catch (err: unknown) {
      logger.warn('Could not list containers for dangling check:', err);
    }
  }

  // ── Internal Helpers ─────────────────────────────────────────────────────────

  /**
   * Collect output from a Docker exec stream, stripping multiplexed frame
   * headers. Docker hijack mode adds 8-byte headers (1 byte type + 3 padding
   * + 4 byte big-endian length) before each frame of stdout/stderr data.
   */
  private static collectDemuxedStream(
    _docker: Docker,
    stream: NodeJS.ReadableStream
  ): Promise<string> {
    return new Promise((resolve, reject) => {
      const chunks: Buffer[] = [];
      stream.on('data', (chunk: Buffer) => chunks.push(chunk));
      stream.on('end', () => {
        const raw = Buffer.concat(chunks);
        // Demux: strip 8-byte frame headers from Docker multiplexed stream
        const textChunks: Buffer[] = [];
        let offset = 0;
        while (offset + 8 <= raw.length) {
          const frameLen = raw.readUInt32BE(offset + 4);
          const frameEnd = offset + 8 + frameLen;
          if (frameEnd > raw.length) break;
          textChunks.push(raw.subarray(offset + 8, frameEnd));
          offset = frameEnd;
        }
        // If no valid frames found, fall back to raw (non-multiplexed stream)
        const result =
          textChunks.length > 0
            ? Buffer.concat(textChunks).toString('utf-8')
            : raw.toString('utf-8');
        resolve(result);
      });
      stream.on('error', reject);
    });
  }

  public audit(operation: string, agentName: string, details: Record<string, unknown>): void {
    logger.info(`${operation.toUpperCase()} | agent=${agentName} | ${JSON.stringify(details)}`);
  }
}
