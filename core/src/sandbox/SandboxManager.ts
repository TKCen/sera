/**
 * SandboxManager — container lifecycle management for SERA agents.
 *
 * Agents never interact with Docker directly. All container operations
 * go through this manager, which validates AGENT.yaml permissions,
 * enforces security tier limits, and logs all operations.
 *
 * @see sera/docs/reimplementation/agent-workspace-architecture.md § Sandbox Manager API
 */

import Docker from 'dockerode';
import { v4 as uuidv4 } from 'uuid';
import type { AgentManifest } from '../agents/manifest/types.js';
import type {
  SandboxInfo,
  SpawnRequest,
  ExecRequest,
  SandboxType,
} from './types.js';
import { TierPolicy, PolicyViolationError } from './TierPolicy.js';

// ── SandboxManager ──────────────────────────────────────────────────────────────

export class SandboxManager {
  private docker: Docker;
  private containers: Map<string, SandboxInfo> = new Map();

  constructor(docker?: Docker) {
    this.docker = docker ?? new Docker({ socketPath: '/var/run/docker.sock' });
  }

  /**
   * Spawn a new sandbox container.
   * Validates agent permissions and applies tier-based resource limits.
   */
  async spawn(manifest: AgentManifest, request: SpawnRequest): Promise<SandboxInfo> {
    const agentName = manifest.metadata.name;

    // ── Permission check ────────────────────────────────────────────────────
    TierPolicy.validateSpawnPermission(manifest, request);

    // ── Subagent instance limit check ───────────────────────────────────────
    if (request.type === 'subagent' && request.subagentRole) {
      const currentCount = this.countSubagents(agentName, request.subagentRole);
      TierPolicy.checkInstanceLimit(manifest, request.subagentRole, currentCount);
    }

    // ── Build container config ──────────────────────────────────────────────
    const limits = TierPolicy.getEffectiveLimits(manifest);
    const containerName = `sera-sandbox-${request.type}-${uuidv4().substring(0, 8)}`;

    const env = Object.entries(request.env ?? {}).map(
      ([k, v]) => `${k}=${v}`,
    );

    // Workspace mount
    const workspacePath = manifest.workspace?.path ?? `/workspaces/${agentName}`;
    const binds = [
      `${workspacePath}:${request.workDir ?? '/workspace'}:${limits.filesystemMode}`,
    ];

    const createOptions: Docker.ContainerCreateOptions = {
      name: containerName,
      Image: request.image,
      Cmd: request.command,
      Env: env,
      WorkingDir: request.workDir ?? '/workspace',
      Labels: {
        'sera.sandbox': 'true',
        'sera.agent': agentName,
        'sera.type': request.type,
        'sera.tier': String(manifest.metadata.tier),
        ...(request.subagentRole ? { 'sera.subagent.role': request.subagentRole } : {}),
        ...(request.task ? { 'sera.task': request.task.substring(0, 200) } : {}),
      },
      HostConfig: {
        CpuShares: limits.cpuShares,
        Memory: limits.memoryBytes,
        NetworkMode: limits.networkMode,
        Binds: binds,
        AutoRemove: request.type === 'tool',
      },
    };

    // ── Create and start ────────────────────────────────────────────────────
    this.audit('spawn', agentName, { type: request.type, image: request.image });

    const container = await this.docker.createContainer(createOptions);
    await container.start();
    const info = await container.inspect();

    const sandboxInfo: SandboxInfo = {
      containerId: info.Id,
      agentName,
      type: request.type,
      image: request.image,
      status: 'running',
      createdAt: new Date().toISOString(),
      tier: manifest.metadata.tier,
      ...(request.type === 'subagent' ? { parentAgent: agentName } : {}),
      ...(request.subagentRole ? { subagentRole: request.subagentRole } : {}),
    };

    this.containers.set(info.Id, sandboxInfo);
    return sandboxInfo;
  }

  /**
   * Execute a command in a running container.
   */
  async exec(manifest: AgentManifest, request: ExecRequest): Promise<{ exitCode: number; output: string }> {
    const sandbox = this.containers.get(request.containerId);
    if (!sandbox) {
      throw new Error(`Container "${request.containerId}" not found in sandbox registry`);
    }

    // Only the owning agent (or parent) can exec into a container
    if (sandbox.agentName !== manifest.metadata.name && sandbox.parentAgent !== manifest.metadata.name) {
      throw new PolicyViolationError(
        `Agent "${manifest.metadata.name}" cannot exec into container owned by "${sandbox.agentName}"`,
        manifest.metadata.name,
        'exec_not_owner',
      );
    }

    this.audit('exec', manifest.metadata.name, { containerId: request.containerId, command: request.command });

    const container = this.docker.getContainer(request.containerId);
    const exec = await container.exec({
      Cmd: request.command,
      AttachStdout: true,
      AttachStderr: true,
    });

    const stream = await exec.start({ hijack: true, stdin: false });
    const output = await SandboxManager.collectStream(stream);

    const inspectResult = await exec.inspect();
    return {
      exitCode: inspectResult.ExitCode ?? -1,
      output,
    };
  }

  /**
   * Remove (stop + delete) a sandbox container.
   */
  async remove(manifest: AgentManifest, containerId: string): Promise<void> {
    const sandbox = this.containers.get(containerId);
    if (!sandbox) {
      throw new Error(`Container "${containerId}" not found in sandbox registry`);
    }

    if (sandbox.agentName !== manifest.metadata.name && sandbox.parentAgent !== manifest.metadata.name) {
      throw new PolicyViolationError(
        `Agent "${manifest.metadata.name}" cannot remove container owned by "${sandbox.agentName}"`,
        manifest.metadata.name,
        'remove_not_owner',
      );
    }

    this.audit('remove', manifest.metadata.name, { containerId });

    sandbox.status = 'removing';

    const container = this.docker.getContainer(containerId);
    try {
      await container.stop({ t: 5 });
    } catch {
      // Container may already be stopped
    }
    try {
      await container.remove({ force: true });
    } catch {
      // Container may have been auto-removed
    }

    this.containers.delete(containerId);
  }

  /**
   * Get logs from a sandbox container.
   */
  async getLogs(containerId: string, tail?: number): Promise<string> {
    const container = this.docker.getContainer(containerId);
    const logs = await container.logs({
      stdout: true,
      stderr: true,
      tail: tail ?? 100,
      follow: false,
    });

    // Logs can be a Buffer or string depending on the TTY setting
    return typeof logs === 'string' ? logs : logs.toString('utf-8');
  }

  /**
   * List all sandbox containers, optionally filtered by agent name.
   */
  listContainers(agentName?: string): SandboxInfo[] {
    const all = Array.from(this.containers.values());
    if (!agentName) return all;
    return all.filter(c => c.agentName === agentName || c.parentAgent === agentName);
  }

  /**
   * Count active subagents of a given role for a parent agent.
   */
  countSubagents(parentAgent: string, role: string): number {
    return Array.from(this.containers.values()).filter(
      c => c.parentAgent === parentAgent && c.subagentRole === role && c.status === 'running',
    ).length;
  }

  // ── Internal Helpers ───────────────────────────────────────────────────────

  /**
   * Collect a Docker stream into a string.
   */
  private static collectStream(stream: NodeJS.ReadableStream): Promise<string> {
    return new Promise((resolve, reject) => {
      const chunks: Buffer[] = [];
      stream.on('data', (chunk: Buffer) => chunks.push(chunk));
      stream.on('end', () => resolve(Buffer.concat(chunks).toString('utf-8')));
      stream.on('error', reject);
    });
  }

  /**
   * Audit trail logging.
   */
  private audit(operation: string, agentName: string, details: Record<string, unknown>): void {
    console.log(
      `[SandboxManager] ${operation.toUpperCase()} | agent=${agentName} | ${JSON.stringify(details)}`,
    );
  }
}
