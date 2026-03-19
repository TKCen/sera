import fs from 'fs';
import path from 'path';
import { execSync } from 'child_process';
import { BaseAgent } from './BaseAgent.js';
import { AgentFactory } from './AgentFactory.js';
import { ProcessManager } from './process/ProcessManager.js';
import type { ProcessType, ProcessTask, ProcessRunResult } from './process/types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import { Logger } from '../lib/logger.js';
import { CapabilityResolver } from '../capability/resolver.js';
import type { AgentRegistry } from './registry.service.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import type { ToolExecutor } from '../tools/ToolExecutor.js';
import type { IdentityService } from '../auth/IdentityService.js';
import type { MeteringEngine } from '../metering/MeteringEngine.js';
import type { AgentScheduler } from '../metering/AgentScheduler.js';

const logger = new Logger('Orchestrator');

// Story 3.6 — agents that miss heartbeats for this long are marked unresponsive
const HEARTBEAT_STALE_MS = parseInt(process.env.HEARTBEAT_STALE_MS ?? '120000', 10);

// Story 3.11 — hard ceiling on subagent recursion depth
const SUBAGENT_MAX_DEPTH = parseInt(process.env.SUBAGENT_MAX_DEPTH ?? '5', 10);

export class RecursionLimitError extends Error {
  constructor(public readonly currentDepth: number, public readonly maxDepth: number) {
    super(`Recursion limit exceeded: depth ${currentDepth} >= max ${maxDepth}`);
    this.name = 'RecursionLimitError';
  }
}

export class Orchestrator {
  private agents: Map<string, BaseAgent> = new Map();
  private manifests: Map<string, AgentManifest> = new Map();
  private primaryAgentName: string | undefined;
  private processManager: ProcessManager = new ProcessManager();
  private intercom: IntercomService | undefined;
  private toolExecutor: ToolExecutor | undefined;
  private sandboxManager: import('../sandbox/SandboxManager.js').SandboxManager | undefined;
  private identityService: IdentityService | undefined;
  private meteringEngine: MeteringEngine | undefined;
  private agentScheduler: AgentScheduler | undefined;
  private registry: AgentRegistry | undefined;
  private heartbeatInterval: NodeJS.Timeout | undefined;
  private cleanupInterval: NodeJS.Timeout | undefined;
  private diskQuotaInterval: NodeJS.Timeout | undefined;

  /** Last heartbeat timestamp per agent instance ID */
  private heartbeats: Map<string, Date> = new Map();

  /** Active file watcher */
  private watcher: fs.FSWatcher | undefined;
  private agentsDir: string | undefined;

  constructor() {
    // Story 3.6 — periodic heartbeat staleness check
    this.heartbeatInterval = setInterval(() => {
      this.checkStaleInstances().catch(err => logger.error('Heartbeat check error:', err));
    }, 30000);

    // Story 3.7 — periodic cleanup of stopped/error containers
    this.cleanupInterval = setInterval(() => {
      this.runCleanupJob().catch(err => logger.error('Cleanup job error:', err));
    }, 60000);

    // Story 3.12 — periodic disk quota check (every 15 min)
    this.diskQuotaInterval = setInterval(() => {
      this.runDiskQuotaCheck().catch(err => logger.error('Disk quota check error:', err));
    }, 15 * 60 * 1000);
  }

  public setRegistry(registry: AgentRegistry): void {
    this.registry = registry;
  }

  public setIntercom(intercom: IntercomService): void {
    this.intercom = intercom;
    for (const agent of this.agents.values()) {
      agent.setIntercom(intercom);
    }
  }

  public setToolExecutor(toolExecutor: ToolExecutor): void {
    this.toolExecutor = toolExecutor;
    for (const agent of this.agents.values()) {
      agent.setToolExecutor(toolExecutor);
    }
  }

  public getToolExecutor(): ToolExecutor | undefined {
    return this.toolExecutor;
  }

  public getManifest(name: string): AgentManifest | undefined {
    return this.manifests.get(name);
  }

  public setSandboxManager(sandboxManager: import('../sandbox/SandboxManager.js').SandboxManager): void {
    this.sandboxManager = sandboxManager;
  }

  public setIdentityService(identityService: IdentityService): void {
    this.identityService = identityService;
  }

  public setMetering(engine: MeteringEngine, scheduler: AgentScheduler): void {
    this.meteringEngine = engine;
    this.agentScheduler = scheduler;
    for (const agent of this.agents.values()) {
      agent.setMetering(engine, scheduler);
    }
  }

  public loadTemplates(dirPath: string): void {
    this.agentsDir = dirPath;
    this.manifests = AgentFactory.loadTemplates(dirPath);
    if (!this.primaryAgentName) {
      const sorted = Array.from(this.manifests.keys()).sort();
      if (sorted.length > 0) this.primaryAgentName = sorted[0];
    }
    logger.info(`Loaded ${this.manifests.size} agent templates`);
  }

  /**
   * Spawn and register an agent instance from the DB.
   * Enforces lifecycle rules and recursion depth before spawning.
   * Stories 3.1–3.3, 3.5, 3.8, 3.11
   */
  public async startInstance(instanceId: string, parentInstanceId?: string): Promise<BaseAgent> {
    if (!this.registry) throw new Error('Registry not configured');

    const instance = await AgentFactory.getInstance(instanceId);
    if (!instance) throw new Error(`Agent instance "${instanceId}" not found`);

    const manifest = this.manifests.get(instance.templateName);
    if (!manifest) throw new Error(`Template "${instance.templateName}" not found`);

    const agent = AgentFactory.createAgent(manifest, instance.id, this.intercom);
    if (this.toolExecutor) agent.setToolExecutor(this.toolExecutor);
    if (this.identityService) agent.setIdentityService(this.identityService);
    if (this.meteringEngine && this.agentScheduler) {
      agent.setMetering(this.meteringEngine, this.agentScheduler);
    }

    // ── Determine lifecycle mode (Story 3.8) ─────────────────────────────
    const templateLifecycle = (manifest as any).spec?.lifecycle?.mode ?? instance.lifecycle_mode ?? 'persistent';
    const resolvedLifecycle: 'persistent' | 'ephemeral' = templateLifecycle;

    // ── Subagent spawn validation ─────────────────────────────────────────
    const effectiveParentId = parentInstanceId ?? instance.parent_instance_id;
    if (effectiveParentId) {
      // Story 3.11: recursion depth guard
      const parentDepth = await this.registry.getLineageDepth(effectiveParentId);
      const childDepth = parentDepth + 1;

      if (childDepth >= SUBAGENT_MAX_DEPTH) {
        const err = new RecursionLimitError(childDepth, SUBAGENT_MAX_DEPTH);
        await this.registry.updateInstanceStatus(instanceId, 'error');
        throw err;
      }

      // Story 3.8: ephemeral parent cannot spawn persistent child
      const parentInstance = await this.registry.getInstance(effectiveParentId);
      if (parentInstance?.lifecycle_mode === 'ephemeral' && resolvedLifecycle === 'persistent') {
        // DECISION: hard guard — force child to ephemeral if parent is ephemeral
        await this.registry.updateInstanceStatus(instanceId, 'error');
        throw new Error(
          `CapabilityEscalationError: ephemeral agent cannot spawn persistent subagent (Story 3.8)`,
        );
      }
    }

    // ── Capability Resolution (Story 3.2) ────────────────────────────────
    const resolver = new CapabilityResolver(this.registry);
    const { resolvedCapabilities } = await resolver.resolve(instance.id);

    // Store resolved capabilities as immutable audit record
    await this.registry.updateInstanceConfig(
      instance.id,
      instance.overrides,
      null,
      resolvedCapabilities,
    );

    // ── Container Spawn (Story 3.1, 3.3) ─────────────────────────────────
    if (this.sandboxManager) {
      try {
        // Issue a 24h JWT for the agent container (Story 3.1)
        let identityToken: string | undefined;
        if (this.identityService) {
          identityToken = this.identityService.signToken(
            {
              agentId: instance.id,
              circleId: (manifest.metadata as any).circle,
              capabilities: resolvedCapabilities,
            },
            '24h',
          );
        }

        const sandbox = await this.sandboxManager.spawn(
          manifest,
          {
            type: 'agent',
            agentName: manifest.metadata.name,
            image: 'sera-agent-worker:latest',
            hostWorkspacePath: instance.workspacePath,
            lifecycleMode: resolvedLifecycle,
            ...(identityToken !== undefined ? { token: identityToken } : {}),
            ...(effectiveParentId !== undefined ? { parentInstanceId: effectiveParentId } : {}),
          },
          resolvedCapabilities,
          instance.id,
        );

        agent.setContainerId(sandbox.containerId);
        await AgentFactory.updateContainerId(instance.id, sandbox.containerId);
        await this.registry.updateInstanceStatus(instance.id, 'running', sandbox.containerId);

        // Story 3.5 — publish lifecycle event to Centrifugo
        this.publishLifecycleEvent('started', instance.id, instance.name, sandbox.containerId);

        logger.info(`Spawned container ${sandbox.containerId} for agent ${instance.name}`);
      } catch (err) {
        await this.registry.updateInstanceStatus(instance.id, 'error');
        // Story 3.5 — publish error event
        this.publishLifecycleEvent('error', instance.id, instance.name);
        logger.error(`Failed to start agent ${instance.name}:`, err);
        throw err;
      }
    }

    this.agents.set(instance.id, agent);
    logger.info(`Started agent instance: ${instance.name} (${instance.id})`);
    return agent;
  }

  /**
   * Stop an agent instance and clean up its container.
   */
  public async stopInstance(instanceId: string): Promise<void> {
    const agent = this.agents.get(instanceId);

    if (this.sandboxManager) {
      let containerId: string | undefined = agent?.containerId;
      let manifest: AgentManifest | undefined = agent?.getManifest();

      if (!containerId || !manifest) {
        const instance = await AgentFactory.getInstance(instanceId);
        containerId = containerId ?? instance?.containerId;
        manifest = manifest ?? (instance ? this.manifests.get(instance.templateName) : undefined);
      }

      if (containerId && manifest) {
        try {
          await this.sandboxManager.remove(manifest, containerId);
          logger.info(`Stopped container ${containerId} for agent ${instanceId}`);
        } catch (err) {
          logger.error(`Failed to stop container ${containerId}:`, err);
        }
      }
    }

    if (this.registry) {
      await this.registry.updateInstanceStatus(instanceId, 'stopped');
    }

    this.agents.delete(instanceId);
    this.heartbeats.delete(instanceId);
    this.publishLifecycleEvent('stopped', instanceId);
    logger.info(`Stopped agent instance: ${instanceId}`);
  }

  // ── Story 3.7: Cleanup ───────────────────────────────────────────────────

  /**
   * Manually trigger cleanup for a specific agent instance.
   * Removes the container but preserves workspace and DB record.
   */
  public async cleanupInstance(instanceId: string): Promise<void> {
    if (this.sandboxManager) {
      await this.sandboxManager.teardown(instanceId);
    }
    if (this.registry) {
      await this.registry.updateInstanceStatus(instanceId, 'stopped');
    }
    this.agents.delete(instanceId);
    this.heartbeats.delete(instanceId);
    this.publishLifecycleEvent('stopped', instanceId);
  }

  /**
   * Background cleanup job: remove containers for stopped/error instances
   * older than the retention period.
   * Story 3.7
   */
  private async runCleanupJob(): Promise<void> {
    if (!this.registry || !this.sandboxManager) return;

    const retentionMs = parseInt(process.env.CONTAINER_RETENTION_MS ?? String(60 * 60 * 1000), 10);
    const cutoff = new Date(Date.now() - retentionMs);

    try {
      const stopped = await this.registry.listInstances({ status: 'stopped' });
      const errored = await this.registry.listInstances({ status: 'error' });

      for (const instance of [...stopped, ...errored]) {
        const updatedAt = new Date(instance.updated_at);
        if (updatedAt < cutoff) {
          await this.sandboxManager.teardown(instance.id).catch(
            err => logger.warn(`Cleanup: failed to teardown ${instance.id}:`, err),
          );
          logger.info(`Cleanup job: removed stale container for instance ${instance.id}`);
        }
      }
    } catch (err) {
      logger.error('Cleanup job error:', err);
    }
  }

  // ── Story 3.6: Heartbeat ─────────────────────────────────────────────────

  public async registerHeartbeat(instanceId: string): Promise<void> {
    this.heartbeats.set(instanceId, new Date());
    if (this.registry) {
      await this.registry.updateLastHeartbeat(instanceId);
    }
  }

  private async checkStaleInstances(): Promise<void> {
    const now = new Date();
    for (const [instanceId, lastHeartbeat] of this.heartbeats.entries()) {
      if (now.getTime() - lastHeartbeat.getTime() > HEARTBEAT_STALE_MS) {
        logger.warn(`Agent instance ${instanceId} has missed heartbeats — marking unresponsive`);
        this.heartbeats.delete(instanceId);
        if (this.registry) {
          await this.registry.updateInstanceStatus(instanceId, 'unresponsive');
        }
        this.publishLifecycleEvent('unresponsive', instanceId);
      }
    }
  }

  public getUnhealthyInstances(timeoutMs: number = HEARTBEAT_STALE_MS): { instanceId: string; lastSeen: Date }[] {
    const now = new Date();
    const unhealthy: { instanceId: string; lastSeen: Date }[] = [];
    for (const [instanceId, lastHeartbeat] of this.heartbeats.entries()) {
      if (now.getTime() - lastHeartbeat.getTime() > timeoutMs) {
        unhealthy.push({ instanceId, lastSeen: lastHeartbeat });
      }
    }
    return unhealthy;
  }

  // ── Story 3.12: Disk Quota ───────────────────────────────────────────────

  private async runDiskQuotaCheck(): Promise<void> {
    if (!this.registry) return;

    const running = await this.registry.listInstances({ status: 'running' });
    const throttled = await this.registry.listInstances({ status: 'throttled' });

    for (const instance of [...running, ...throttled]) {
      const caps = instance.resolved_capabilities;
      const limitGB: number | undefined = caps?.filesystem?.maxWorkspaceSizeGB;

      const workspacePath = instance.workspace_path;
      if (!workspacePath) continue;

      // Log startup warning if no limit is set and agent has write access (Story 3.12)
      if (!limitGB && caps?.filesystem?.write) {
        logger.warn(`Agent ${instance.name} has filesystem.write but no maxWorkspaceSizeGB — no quota enforced`);
        continue;
      }

      if (!limitGB) continue;

      let usedGB = 0;
      try {
        const duOutput = execSync(`du -s --block-size=1G "${workspacePath}" 2>/dev/null || echo "0"`, {
          encoding: 'utf-8',
          shell: '/bin/sh',
        });
        usedGB = parseInt(duOutput.split('\t')[0] ?? '0', 10) || 0;
      } catch {
        // du not available (Windows dev env) — skip
        continue;
      }

      await this.registry.updateWorkspaceUsage(instance.id, usedGB);

      const isEphemeral = instance.lifecycle_mode === 'ephemeral';

      if (usedGB >= limitGB) {
        if (!isEphemeral || instance.status !== 'throttled') {
          await this.registry.updateInstanceStatus(instance.id, 'throttled');
          this.publishLifecycleEvent('throttled', instance.id, instance.name);
          logger.warn(`Agent ${instance.name} exceeded disk quota: ${usedGB}GB / ${limitGB}GB`);
        }
      } else if (instance.status === 'throttled') {
        // Usage dropped below limit — restore to running
        await this.registry.updateInstanceStatus(instance.id, 'running');
        this.publishLifecycleEvent('running', instance.id, instance.name);
        logger.info(`Agent ${instance.name} usage back within quota: ${usedGB}GB / ${limitGB}GB`);
      }
    }
  }

  // ── Docker Events (Story 3.5) ────────────────────────────────────────────

  public async startDockerEventListener(): Promise<void> {
    if (!this.sandboxManager) return;

    await this.sandboxManager.startEventListener(async (event) => {
      if (!this.registry) return;

      const { action, instanceId, exitCode } = event;

      if (action === 'start') {
        await this.registry.updateInstanceStatus(instanceId, 'running');
      } else if (action === 'die') {
        const status = exitCode !== undefined && exitCode !== 0 ? 'error' : 'stopped';
        await this.registry.updateInstanceStatus(instanceId, status);
        this.heartbeats.delete(instanceId);
        this.publishLifecycleEvent(status as any, instanceId, event.agentName);
      } else if (action === 'oom') {
        await this.registry.updateInstanceStatus(instanceId, 'error');
        this.heartbeats.delete(instanceId);
        logger.warn(`OOM kill: agent=${event.agentName} instance=${instanceId}`);
        this.publishLifecycleEvent('error', instanceId, event.agentName);
      } else if (action === 'stop') {
        await this.registry.updateInstanceStatus(instanceId, 'stopped');
        this.heartbeats.delete(instanceId);
      }
    });

    // Story 3.5 — warn about dangling containers on startup
    if (this.registry) {
      const instances = await this.registry.listInstances();
      const knownIds = new Set(instances.map((i: any) => i.id as string));
      await this.sandboxManager.checkDanglingContainers(knownIds);
    }
  }

  // ── Centrifugo publish ───────────────────────────────────────────────────

  private publishLifecycleEvent(
    type: string,
    instanceId: string,
    agentName?: string,
    containerId?: string,
  ): void {
    if (!this.intercom) return;
    this.intercom.publish('system.agents', {
      type,
      agentId: instanceId,
      agentName,
      containerId,
      timestamp: new Date().toISOString(),
    }).catch(err => logger.error('Failed to publish lifecycle event:', err));
  }

  // ── Template / Agent Management ─────────────────────────────────────────

  public watchAgentsDirectory(dirPath: string): void {
    if (this.watcher) this.watcher.close();
    this.agentsDir = dirPath;
    this.watcher = fs.watch(dirPath, { recursive: true }, (eventType, filename) => {
      if (filename?.endsWith('AGENT.yaml')) {
        logger.info(`Agent template change detected: ${filename}`);
        this.loadTemplates(dirPath);
      }
    });
    logger.info(`Watching for agent changes in ${dirPath}`);
  }

  public registerAgent(agent: BaseAgent): void {
    const id = agent.agentInstanceId ?? agent.name;
    this.agents.set(id, agent);
  }

  public getAgent(id: string): BaseAgent | undefined {
    return this.agents.get(id);
  }

  public getAllManifests(): AgentManifest[] {
    return Array.from(this.manifests.values());
  }

  public reloadTemplates(): { count: number } {
    if (!this.agentsDir) throw new Error('No agents directory configured');
    this.manifests = AgentFactory.loadTemplates(this.agentsDir);
    logger.info(`Reloaded ${this.manifests.size} agent templates`);
    return { count: this.manifests.size };
  }

  public stopWatching(): void {
    this.stop().catch(err => logger.error('Error stopping orchestrator:', err));
  }

  public async stop(): Promise<void> {
    if (this.heartbeatInterval) clearInterval(this.heartbeatInterval);
    if (this.cleanupInterval) clearInterval(this.cleanupInterval);
    if (this.diskQuotaInterval) clearInterval(this.diskQuotaInterval);
    if (this.watcher) {
      this.watcher.close();
      this.watcher = undefined;
    }
  }

  public setPrimaryAgent(name: string): void {
    this.primaryAgentName = name;
  }

  public getPrimaryAgent(): BaseAgent | undefined {
    if (this.primaryAgentName) {
      const agent = this.agents.get(this.primaryAgentName);
      if (agent) return agent;
      const found = Array.from(this.agents.values()).find(
        a => a.getManifest().metadata.name === this.primaryAgentName,
      );
      if (found) return found;
    }
    return Array.from(this.agents.values())[0];
  }

  public listAgents(): any[] {
    return Array.from(this.manifests.values()).map(m => ({
      name: m.metadata.name,
      displayName: m.metadata.displayName,
      role: m.identity.role,
      tier: (m.metadata as any).tier,
    }));
  }

  public getAgentInfo(name: string): any {
    const manifest = this.manifests.get(name);
    if (!manifest) return undefined;
    return { name: manifest.metadata.name, manifest };
  }

  public updateLlmProvider(llmProvider: LLMProvider) {
    for (const agent of this.agents.values()) {
      agent.updateLlmProvider(llmProvider);
    }
  }

  async executeTask(description: string) {
    const primaryAgent = this.getPrimaryAgent();
    if (!primaryAgent) throw new Error('No primary agent configured');
    const result = await this.processManager.runSingle(description, primaryAgent);
    return result.finalOutput ?? 'No answer provided.';
  }

  async executeWithProcess(type: ProcessType, tasks: ProcessTask[], managerAgentName?: string): Promise<ProcessRunResult> {
    const managerAgent = managerAgentName ? this.agents.get(managerAgentName) : undefined;
    return this.processManager.run(type, tasks, this.agents, managerAgent);
  }
}
