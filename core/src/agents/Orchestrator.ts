import fs from 'fs';
import { execSync } from 'child_process';
import { BaseAgent } from './BaseAgent.js';
import { AgentFactory } from './AgentFactory.js';
import { ProcessManager } from './process/ProcessManager.js';
import type { ProcessType, ProcessTask, ProcessRunResult } from './process/types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest, ResolvedCapabilities } from './manifest/types.js';
import type { LlmRouter } from '../llm/LlmRouter.js';
import { Logger } from '../lib/logger.js';
import { CapabilityResolver } from '../capability/resolver.js';
import type { AgentRegistry } from './registry.service.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import { ToolExecutor } from '../tools/ToolExecutor.js';
import { IdentityService } from '../auth/IdentityService.js';
import { MeteringEngine } from '../metering/MeteringEngine.js';
import type { AgentScheduler } from '../metering/AgentScheduler.js';
import { query } from '../lib/database.js';
import { AuditService } from '../audit/AuditService.js';

const logger = new Logger('Orchestrator');

// Story 3.6 — agents that miss heartbeats for this long are marked unresponsive
const HEARTBEAT_STALE_MS = parseInt(process.env.HEARTBEAT_STALE_MS ?? '120000', 10);

// Story 3.11 — hard ceiling on subagent recursion depth
const SUBAGENT_MAX_DEPTH = parseInt(process.env.SUBAGENT_MAX_DEPTH ?? '5', 10);

export class RecursionLimitError extends Error {
  constructor(
    public readonly currentDepth: number,
    public readonly maxDepth: number
  ) {
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
  private llmRouter: LlmRouter | undefined;
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
      this.checkStaleInstances().catch((err) => logger.error('Heartbeat check error:', err));
    }, 30000);

    // Story 3.7 — periodic cleanup of stopped/error containers
    this.cleanupInterval = setInterval(() => {
      this.runCleanupJob().catch((err) => logger.error('Cleanup job error:', err));
    }, 60000);

    // Story 3.12 — periodic disk quota check (every 15 min)
    this.diskQuotaInterval = setInterval(
      () => {
        this.runDiskQuotaCheck().catch((err) => logger.error('Disk quota check error:', err));
      },
      15 * 60 * 1000
    );
  }

  public setRegistry(registry: AgentRegistry): void {
    this.registry = registry;
  }

  public setLlmRouter(router: LlmRouter): void {
    this.llmRouter = router;
  }

  public setIntercom(intercom: IntercomService): void {
    this.intercom = intercom;
    for (const agent of this.agents.values()) {
      agent.setIntercom(intercom);
    }
  }

  public getIntercom(): IntercomService | undefined {
    return this.intercom;
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

  public getManifestByInstanceId(instanceId: string): AgentManifest | undefined {
    return this.agents.get(instanceId)?.getManifest();
  }

  public setSandboxManager(
    sandboxManager: import('../sandbox/SandboxManager.js').SandboxManager
  ): void {
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
  public async startInstance(
    instanceId: string,
    parentInstanceId?: string,
    task?: string
  ): Promise<BaseAgent> {
    if (!this.registry) throw new Error('Registry not configured');

    const instance = await AgentFactory.getInstance(instanceId);
    if (!instance) throw new Error(`Agent instance "${instanceId}" not found`);

    const templateRow = await this.registry.getTemplate(instance.template_ref);
    if (!templateRow) {
      throw new Error(
        `Template "${instance.template_ref}" not found for instance "${instance.name}" (${instanceId})`
      );
    }

    // Convert the DB template row into an AgentManifest shape.
    // getTemplate() returns a raw DB row ({ name, display_name, spec, ... })
    // but AgentFactory.createAgent expects { metadata, identity, model, spec }.
    const manifest: AgentManifest = {
      apiVersion: 'sera/v1',
      kind: 'Agent',
      metadata: {
        name: templateRow.name,
        displayName: templateRow.display_name ?? templateRow.name,
        icon: templateRow.spec?.identity?.icon ?? '',
        circle: templateRow.spec?.circle ?? instance.circle,
        tier:
          templateRow.spec?.sandboxBoundary === 'tier-3'
            ? 3
            : templateRow.spec?.sandboxBoundary === 'tier-2'
              ? 2
              : 1,
      },
      identity: {
        role: templateRow.spec?.identity?.role ?? templateRow.name,
        description: templateRow.spec?.identity?.description ?? '',
        communicationStyle: templateRow.spec?.identity?.communicationStyle,
        principles: templateRow.spec?.identity?.principles,
      },
      model: {
        provider: templateRow.spec?.model?.provider ?? 'default',
        name: templateRow.spec?.model?.name ?? 'default',
        temperature: templateRow.spec?.model?.temperature,
        fallback: templateRow.spec?.model?.fallback,
      },
      spec: templateRow.spec ?? {},
    };

    // ── Apply instance overrides (model, sandboxBoundary, resources) ──────
    const overrides = (instance.overrides ?? {}) as Record<string, unknown>;
    const modelOv = overrides.model as Record<string, unknown> | undefined;
    if (modelOv) {
      if (modelOv.name) manifest.model.name = modelOv.name as string;
      if (modelOv.provider) manifest.model.provider = modelOv.provider as string;
      if (modelOv.temperature !== undefined) {
        manifest.model.temperature = modelOv.temperature as number;
      }
    }
    if (overrides.sandboxBoundary) {
      manifest.spec = {
        ...(manifest.spec ?? {}),
        sandboxBoundary: overrides.sandboxBoundary as string,
      };
      // Also update metadata tier for consistency
      const sb = overrides.sandboxBoundary as string;
      manifest.metadata.tier = sb === 'tier-3' ? 3 : sb === 'tier-2' ? 2 : 1;
    }
    const resourcesOv = overrides.resources as Record<string, unknown> | undefined;
    if (resourcesOv) {
      manifest.spec = {
        ...(manifest.spec ?? {}),
        resources: {
          ...((manifest.spec?.resources as Record<string, unknown>) ?? {}),
          ...resourcesOv,
        },
      };
    }

    this.manifests.set(instance.template_ref, manifest);

    const agent = AgentFactory.createAgent(manifest, instance.id, this.intercom, this.llmRouter);
    if (this.toolExecutor) agent.setToolExecutor(this.toolExecutor);
    if (this.identityService) agent.setIdentityService(this.identityService);
    if (this.meteringEngine && this.agentScheduler) {
      agent.setMetering(this.meteringEngine, this.agentScheduler);
    }

    // ── Determine lifecycle mode (Story 3.8) ─────────────────────────────
    const templateLifecycle =
      manifest.spec?.lifecycle?.mode ?? instance.lifecycle_mode ?? 'persistent';
    const resolvedLifecycle: 'persistent' | 'ephemeral' = templateLifecycle as
      | 'persistent'
      | 'ephemeral';

    // ── Subagent spawn validation & Circle Inheritance ───────────────────
    const effectiveParentId = parentInstanceId ?? instance.parent_instance_id;
    let resolvedCircleId = instance.circle_id;

    if (effectiveParentId) {
      // Story 3.11: recursion depth guard
      const parentDepth = await this.registry.getLineageDepth(effectiveParentId);
      const childDepth = parentDepth + 1;

      if (childDepth >= SUBAGENT_MAX_DEPTH) {
        // Record denial in audit trail
        AuditService.getInstance()
          ?.record({
            actorType: 'agent',
            actorId: effectiveParentId,
            actingContext: null,
            eventType: 'agent.spawn.denied',
            payload: {
              reason: 'recursion_limit',
              callerAgentId: effectiveParentId,
              targetInstanceId: instanceId,
              depth: childDepth,
              maxDepth: SUBAGENT_MAX_DEPTH,
            },
          })
          .catch((e: unknown) => logger.warn('Failed to record recursion limit audit:', e));

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
          `CapabilityEscalationError: ephemeral agent cannot spawn persistent subagent (Story 3.8)`
        );
      }

      // Story 10.4: circle inheritance
      if (!resolvedCircleId && parentInstance?.circle_id) {
        resolvedCircleId = parentInstance.circle_id;
        // Persist the inherited circle ID
        await query(`UPDATE agent_instances SET circle_id = $1 WHERE id = $2`, [
          resolvedCircleId,
          instanceId,
        ]);
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
      resolvedCapabilities
    );

    // ── Container Spawn (Story 3.1, 3.3) ─────────────────────────────────
    if (this.sandboxManager) {
      try {
        // Issue a 24h JWT for the agent container (Story 3.1)
        let identityToken: string | undefined;
        if (this.identityService) {
          identityToken = await this.identityService.signToken(
            {
              agentId: instance.id,
              agentName: manifest.metadata.name,
              circleId: (resolvedCircleId || manifest.metadata.circle || '') as string,
              capabilities: resolvedCapabilities as Record<string, unknown>,
              scope: 'agent',
            },
            '24h'
          );
        }

        // Story 16.9 — load agent-env secrets (exposure: agent-env only)
        const agentEnvSecrets: Record<string, string> = {};
        const capabilities = resolvedCapabilities as ResolvedCapabilities;
        // const limitGB = capabilities?.filesystem?.maxWorkspaceSizeGB || 5;
        const canWrite = capabilities?.filesystem?.write !== false;

        if (!canWrite) {
          // TODO: Implement read-only workspace
        }
        const secretNames: string[] = Array.isArray(capabilities?.secrets?.access)
          ? (capabilities.secrets.access as string[])
          : [];

        if (secretNames.length > 0) {
          const { SecretsManager } = await import('../secrets/secrets-manager.js');
          for (const secretName of secretNames) {
            try {
              const secretValue = await SecretsManager.getInstance().get(secretName, {
                agentId: instance.id,
                agentName: instance.name,
              });
              if (secretValue !== null) {
                // Fetch exposure metadata to determine injection mode
                const { query: dbQuery } = await import('../lib/database.js');
                const metaResult = await dbQuery(
                  'SELECT exposure FROM secrets WHERE name = $1 AND deleted_at IS NULL',
                  [secretName]
                );
                const exposure = metaResult.rows[0]?.exposure ?? 'per-call';
                if (exposure === 'agent-env') {
                  agentEnvSecrets[secretName] = secretValue;
                  await AuditService.getInstance()
                    .record({
                      actorType: 'system',
                      actorId: 'system',
                      actingContext: null,
                      eventType: 'secret.injected',
                      payload: { secretName, agentId: instance.id },
                    })
                    .catch(() => {});
                }
              }
            } catch (secretErr) {
              logger.warn(`Secret "${secretName}" denied for agent ${instance.name}:`, secretErr);
            }
          }
        }

        const sandbox = await this.sandboxManager.spawn(
          manifest,
          {
            type: 'agent',
            agentName: manifest.metadata.name,
            image: 'sera-agent-worker:latest',
            ...(instance.workspace_path ? { hostWorkspacePath: instance.workspace_path } : {}),
            lifecycleMode: resolvedLifecycle,
            ...(task !== undefined ? { task } : {}),
            ...(identityToken !== undefined ? { token: identityToken } : {}),
            ...(effectiveParentId !== undefined ? { parentInstanceId: effectiveParentId } : {}),
          },
          resolvedCapabilities,
          instance.id,
          Object.keys(agentEnvSecrets).length > 0 ? agentEnvSecrets : undefined
        );

        // Zero out in-memory secret values after spawn (Story 16.9)
        for (const key of Object.keys(agentEnvSecrets)) {
          agentEnvSecrets[key] = '\0'.repeat(agentEnvSecrets[key]!.length);
          delete agentEnvSecrets[key];
        }

        agent.setContainerId(sandbox.containerId);
        await AgentFactory.updateContainerId(instance.id, sandbox.containerId);
        await this.registry.updateInstanceStatus(instance.id, 'running', sandbox.containerId);

        // Story 3.5 — publish lifecycle event to Centrifugo
        this.publishLifecycleEvent('started', instance.id, instance.name, sandbox.containerId);

        await AuditService.getInstance().record({
          actorType: 'system',
          actorId: 'system',
          actingContext: null,
          eventType: 'agent.spawned',
          payload: {
            agentId: instance.id,
            agentName: instance.name,
            containerId: sandbox.containerId,
          },
        });

        logger.info(`Spawned container ${sandbox.containerId} for agent ${instance.name}`);
      } catch (err) {
        await this.registry.updateInstanceStatus(instance.id, 'error');
        // Story 3.5 — publish error event
        this.publishLifecycleEvent('error', instance.id, instance.name);

        await AuditService.getInstance().record({
          actorType: 'system',
          actorId: 'system',
          actingContext: null,
          eventType: 'agent.crashed',
          payload: {
            agentId: instance.id,
            agentName: instance.name,
            error: (err as Error).message,
          },
        });

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
        containerId = containerId ?? instance?.container_id;
        manifest = manifest ?? (instance ? this.manifests.get(instance.template_ref) : undefined);
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

    await AuditService.getInstance().record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'agent.stopped',
      payload: { agentId: instanceId },
    });

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
        const lastUpdate = instance.updated_at ? new Date(instance.updated_at).getTime() : 0;
        if (lastUpdate < cutoff.getTime()) {
          await this.sandboxManager
            .teardown(instance.id)
            .catch((err) => logger.warn(`Cleanup: failed to teardown ${instance.id}:`, err));
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

  public getUnhealthyInstances(
    timeoutMs: number = HEARTBEAT_STALE_MS
  ): { instanceId: string; lastSeen: Date }[] {
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
      const caps = instance.resolved_capabilities as ResolvedCapabilities;
      const limitGB: number | undefined = caps?.filesystem?.maxWorkspaceSizeGB;

      const workspacePath = instance.workspace_path;
      if (!workspacePath) continue;

      // Log startup warning if no limit is set and agent has write access (Story 3.12)
      if (!limitGB && caps?.filesystem?.write) {
        logger.warn(
          `Agent ${instance.name} has filesystem.write but no maxWorkspaceSizeGB — no quota enforced`
        );
        continue;
      }

      if (!limitGB) continue;

      let usedGB = 0;
      try {
        const duOutput = execSync(
          `du -s --block-size=1G "${workspacePath}" 2>/dev/null || echo "0"`,
          {
            encoding: 'utf-8',
            shell: '/bin/sh',
          }
        );
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
      } else if ((instance.status as string) === 'throttled') {
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
        const agent = this.agents.get(instanceId);
        if (agent) agent.status = 'running';
        await this.registry.updateInstanceStatus(instanceId, 'running');
      } else if (action === 'die') {
        const status = exitCode !== undefined && exitCode !== 0 ? 'error' : 'stopped';
        const agent = this.agents.get(instanceId);
        if (agent) agent.status = status as 'error' | 'stopped';
        await this.registry.updateInstanceStatus(instanceId, status);
        this.heartbeats.delete(instanceId);
        this.publishLifecycleEvent(status, instanceId, event.agentName);
      } else if (action === 'oom') {
        const agent = this.agents.get(instanceId);
        if (agent) agent.status = 'error';
        await this.registry.updateInstanceStatus(instanceId, 'error');
        this.heartbeats.delete(instanceId);
        logger.warn(`OOM kill: agent=${event.agentName} instance=${instanceId}`);
        this.publishLifecycleEvent('error', instanceId, event.agentName);
      } else if (action === 'stop') {
        const agent = this.agents.get(instanceId);
        if (agent) agent.status = 'stopped';
        await this.registry.updateInstanceStatus(instanceId, 'stopped');
        this.heartbeats.delete(instanceId);
      }
    });

    // Story 3.5 — warn about dangling containers on startup
    if (this.registry) {
      const instances = await this.registry.listInstances();
      const knownIds = new Set(instances.map((i) => i.id));
      await this.sandboxManager.checkDanglingContainers(knownIds);
    }
  }

  // ── Centrifugo publish ───────────────────────────────────────────────────

  private publishLifecycleEvent(
    type: string,
    instanceId: string,
    agentName?: string,
    containerId?: string
  ): void {
    if (!this.intercom) return;
    this.intercom
      .publish('system.agents', {
        type,
        agentId: instanceId,
        agentName,
        containerId,
        timestamp: new Date().toISOString(),
      })
      .catch((err) => logger.error('Failed to publish lifecycle event:', err));
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
    // Use the stable manifest name (metadata.name) as the key so that
    // getAgent(manifestName) always resolves correctly. Fall back to the
    // display name only when an instance ID is unavailable and the role
    // is also missing (shouldn't happen in practice).
    const id = agent.agentInstanceId ?? agent.role ?? agent.name;
    this.agents.set(id, agent);
  }

  public getAgent(id: string): BaseAgent | undefined {
    return this.agents.get(id);
  }

  public getAllManifests(): AgentManifest[] {
    return Array.from(this.manifests.values());
  }

  public reloadTemplates(): {
    count: number;
    added: string[];
    updated: string[];
    removed: string[];
  } {
    if (!this.agentsDir) throw new Error('No agents directory configured');
    const oldKeys = new Set(this.manifests.keys());
    this.manifests = AgentFactory.loadTemplates(this.agentsDir);
    const newKeys = new Set(this.manifests.keys());

    const added = Array.from(newKeys).filter((k) => !oldKeys.has(k));
    const updated = Array.from(newKeys).filter((k) => oldKeys.has(k));
    const removed = Array.from(oldKeys).filter((k) => !newKeys.has(k));

    logger.info(
      `Reloaded ${this.manifests.size} agent templates (added=${added.length}, updated=${updated.length}, removed=${removed.length})`
    );
    return {
      count: this.manifests.size,
      added,
      updated,
      removed,
    };
  }

  public stopWatching(): void {
    this.stop().catch((err) => logger.error('Error stopping orchestrator:', err));
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
        (a) => a.getManifest().metadata.name === this.primaryAgentName
      );
      if (found) return found;
    }
    return Array.from(this.agents.values())[0];
  }

  public deregisterAgent(name: string): void {
    // Remove manifest
    this.manifests.delete(name);
    // Remove all agent instances that belong to this manifest name
    for (const [key, agent] of this.agents.entries()) {
      if (agent.getManifest().metadata.name === name) {
        this.agents.delete(key);
      }
    }
    logger.info(`Deregistered agent "${name}"`);
  }

  public listAgents(): { id?: string; name: string; status: string; startTime: Date }[] {
    const active = Array.from(this.agents.values()).map((a) => ({
      id: a.agentInstanceId || '',
      name: a.getManifest().metadata.name,
      status: a.status as string,
      startTime: a.startTime,
    }));
    return active;
  }

  public async restartAgent(instanceId: string): Promise<void> {
    logger.info(`Restarting agent instance: ${instanceId}`);
    await this.stopInstance(instanceId);
    await this.startInstance(instanceId);
  }

  public getAgentInfo(name: string): { name: string; manifest: AgentManifest } | undefined {
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

  async executeWithProcess(
    type: ProcessType,
    tasks: ProcessTask[],
    managerAgentName?: string
  ): Promise<ProcessRunResult> {
    const managerAgent = managerAgentName ? this.agents.get(managerAgentName) : undefined;
    return this.processManager.run(type, tasks, this.agents, managerAgent);
  }
}
