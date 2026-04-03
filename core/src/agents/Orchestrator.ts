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
import type { ContextCompactionService } from '../llm/ContextCompactionService.js';
import type { HeartbeatService } from './HeartbeatService.js';
import type { CleanupService } from './CleanupService.js';
import type { DiskQuotaService } from './DiskQuotaService.js';

const logger = new Logger('Orchestrator');

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
  private contextCompactionService: ContextCompactionService | undefined;
  private registry: AgentRegistry | undefined;
  private llmRouter: LlmRouter | undefined;
  private heartbeatService: HeartbeatService | undefined;
  private cleanupService: CleanupService | undefined;
  private diskQuotaService: DiskQuotaService | undefined;
  private circleContextResolver: ((circleName: string) => string | undefined) | undefined;

  /** Active file watcher */
  private watcher: fs.FSWatcher | undefined;
  private agentsDir: string | undefined;

  constructor() {}

  public setRegistry(registry: AgentRegistry): void {
    this.registry = registry;
  }

  public setLlmRouter(router: LlmRouter): void {
    this.llmRouter = router;
  }

  public setHeartbeatService(service: HeartbeatService): void {
    this.heartbeatService = service;
  }

  public setCleanupService(service: CleanupService): void {
    this.cleanupService = service;
  }

  public setDiskQuotaService(service: DiskQuotaService): void {
    this.diskQuotaService = service;
  }

  public setCircleContextResolver(resolver: (circleName: string) => string | undefined): void {
    this.circleContextResolver = resolver;
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

  public setContextCompactionService(service: ContextCompactionService): void {
    this.contextCompactionService = service;
    for (const agent of this.agents.values()) {
      agent.setContextCompactionService(service);
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

    // Convert the DB template row or instance snapshot into an AgentManifest shape.
    // getTemplate() returns a raw DB row ({ name, display_name, spec, ... })
    // but AgentFactory.createAgent expects { metadata, identity, model, spec }.
    // We use the instance's resolved_config as the base spec to ensure stability
    // and operator awareness of template updates.
    const s = (instance.resolved_config as Record<string, any>) ?? templateRow.spec ?? {};
    const manifest: AgentManifest = {
      apiVersion: 'sera/v1',
      kind: 'Agent',
      metadata: {
        name: templateRow.name,
        displayName: templateRow.display_name ?? templateRow.name,
        icon: s.identity?.icon ?? '',
        circle: s.circle ?? instance.circle ?? '',
        tier: s.sandboxBoundary === 'tier-3' ? 3 : s.sandboxBoundary === 'tier-2' ? 2 : 1,
      },
      identity: {
        role: s.identity?.role ?? templateRow.name,
        description: s.identity?.description ?? '',
        communicationStyle: s.identity?.communicationStyle,
        principles: s.identity?.principles,
      },
      model: {
        provider: s.model?.provider ?? 'default',
        name: s.model?.name ?? 'default',
        temperature: s.model?.temperature,
        fallback: s.model?.fallback,
      },
      spec: s,
    };

    // Normalize spec-wrapped fields to top-level so consumers don't need
    // to check both manifest.X and manifest.spec.X (#539)
    const m = manifest as unknown as Record<string, unknown>;
    if (s.tools) m['tools'] = s.tools;
    if (s.skills) m['skills'] = s.skills;
    if (s.skillPackages) m['skillPackages'] = s.skillPackages;
    if (s.subagents) m['subagents'] = s.subagents;
    if (s.resources) m['resources'] = s.resources;
    if (s.workspace) m['workspace'] = s.workspace;
    if (s.memory) m['memory'] = s.memory;
    if (s.permissions) m['permissions'] = s.permissions;
    if (s.capabilities) m['capabilities'] = s.capabilities;
    if (s.schedules) m['schedules'] = s.schedules;

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
    // Apply tools overrides (allowed/denied/coreTools lists)
    const toolsOv = overrides.tools as Record<string, unknown> | undefined;
    if (toolsOv) {
      const baseTools = (manifest.spec?.tools as Record<string, unknown>) ?? {};
      const mergedTools = { ...baseTools, ...toolsOv };
      manifest.spec = { ...(manifest.spec ?? {}), tools: mergedTools };
      (manifest as unknown as Record<string, unknown>)['tools'] = mergedTools;
    }
    // Apply permissions overrides
    const permOv = overrides.permissions as Record<string, unknown> | undefined;
    if (permOv) {
      manifest.spec = {
        ...(manifest.spec ?? {}),
        permissions: {
          ...((manifest.spec?.permissions as Record<string, unknown>) ?? {}),
          ...permOv,
        },
      };
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

    // Validate that tools.allowed patterns match registered tools (Story #524)
    if (this.toolExecutor) {
      this.toolExecutor.validateToolPatterns(manifest);
    }

    const agent = AgentFactory.createAgent(manifest, instance.id, this.intercom, this.llmRouter);
    if (this.toolExecutor) agent.setToolExecutor(this.toolExecutor);
    if (this.identityService) agent.setIdentityService(this.identityService);
    if (this.meteringEngine && this.agentScheduler) {
      agent.setMetering(this.meteringEngine, this.agentScheduler);
    }
    if (this.contextCompactionService) {
      agent.setContextCompactionService(this.contextCompactionService);
    }

    // Wire circle context resolver so agents can access their circle's shared context
    if (this.circleContextResolver && manifest.metadata.circle) {
      const circleName = manifest.metadata.circle;
      const resolver = this.circleContextResolver;
      agent.setCircleContextResolver(() => resolver(circleName));
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

    // Store resolved capabilities as immutable audit record.
    // Preserve the current resolved_config baseline.
    await this.registry.updateInstanceConfig(
      instance.id,
      instance.overrides,
      instance.resolved_config,
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
          logger.info(`Agent ${instance.name} (${instance.id}) workspace configured as read-only`);
          await AuditService.getInstance()
            .record({
              actorType: 'system',
              actorId: 'system',
              actingContext: null,
              eventType: 'agent.workspace.readonly',
              payload: { agentId: instance.id },
            })
            .catch((e: unknown) => logger.warn('Failed to record readonly workspace audit:', e));
        }
        const secretNames: string[] = Array.isArray(capabilities?.secrets?.access)
          ? (capabilities.secrets.access as string[])
          : [];

        if (secretNames.length > 0) {
          const [{ SecretsManager }, { query: dbQuery }] = await Promise.all([
            import('../secrets/secrets-manager.js'),
            import('../lib/database.js'),
          ]);

          await Promise.all(
            secretNames.map(async (secretName) => {
              try {
                const secretValue = await SecretsManager.getInstance().get(secretName, {
                  agentId: instance.id,
                  agentName: instance.name,
                });
                if (secretValue !== null) {
                  // Fetch exposure metadata to determine injection mode
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
            })
          );
        }

        // Resolve model context window from provider config (for agent-runtime ContextManager)
        const spawnEnv: Record<string, string> = {};
        const modelName =
          (manifest.model?.name ?? (manifest.spec as Record<string, unknown> | undefined)?.model)
            ? (((manifest.spec as Record<string, unknown>)?.model as Record<string, unknown>)
                ?.name as string | undefined)
            : undefined;
        if (modelName && this.llmRouter) {
          try {
            const providerConfig = this.llmRouter.getRegistry().resolve(modelName);
            if (providerConfig.contextWindow) {
              spawnEnv['CONTEXT_WINDOW'] = String(providerConfig.contextWindow);
            }
            if (providerConfig.contextStrategy) {
              spawnEnv['CONTEXT_COMPACTION_STRATEGY'] = providerConfig.contextStrategy;
            }
          } catch {
            // Model not in registry — agent-runtime will use its defaults
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
            ...(Object.keys(spawnEnv).length > 0 ? { env: spawnEnv } : {}),
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
    if (this.heartbeatService) {
      this.heartbeatService.removeHeartbeat(instanceId);
    }
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
    if (this.heartbeatService) {
      this.heartbeatService.removeHeartbeat(instanceId);
    }
    this.publishLifecycleEvent('stopped', instanceId);
  }

  // ── Heartbeat Delegations ────────────────────────────────────────────────

  public async registerHeartbeat(instanceId: string): Promise<void> {
    if (this.heartbeatService) {
      await this.heartbeatService.registerHeartbeat(instanceId);
    }
  }

  public getUnhealthyInstances(timeoutMs?: number): { instanceId: string; lastSeen: Date }[] {
    if (this.heartbeatService) {
      return this.heartbeatService.getUnhealthyInstances(timeoutMs);
    }
    return [];
  }

  // ── Story 3.12: Disk Quota ───────────────────────────────────────────────

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
        if (this.heartbeatService) {
          this.heartbeatService.removeHeartbeat(instanceId);
        }
        this.publishLifecycleEvent(status, instanceId, event.agentName);
      } else if (action === 'oom') {
        const agent = this.agents.get(instanceId);
        if (agent) agent.status = 'error';
        await this.registry.updateInstanceStatus(instanceId, 'error');
        if (this.heartbeatService) {
          this.heartbeatService.removeHeartbeat(instanceId);
        }
        logger.warn(`OOM kill: agent=${event.agentName} instance=${instanceId}`);
        this.publishLifecycleEvent('error', instanceId, event.agentName);
      } else if (action === 'stop') {
        const agent = this.agents.get(instanceId);
        if (agent) agent.status = 'stopped';
        await this.registry.updateInstanceStatus(instanceId, 'stopped');
        if (this.heartbeatService) {
          this.heartbeatService.removeHeartbeat(instanceId);
        }
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

  /**
   * Publish an agent lifecycle event to Centrifugo.
   * Story 3.5
   */
  public publishLifecycleEvent(
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

  /** Get all running agents (for tools usedBy enrichment). */
  public getRunningAgents(): Map<string, BaseAgent> {
    return this.agents;
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
    if (this.cleanupService) this.cleanupService.stop();
    if (this.diskQuotaService) this.diskQuotaService.stop();
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

  /**
   * Ensure an agent instance has a running container with a reachable chat
   * server.  If the container is not running, starts the instance and waits
   * for the chat server to become ready (via the spawn readiness poll).
   *
   * @returns The chat URL to forward requests to.
   * @throws  If the container cannot be started or the chat URL is unavailable.
   */
  public async ensureContainerRunning(instanceId: string): Promise<string> {
    if (!this.sandboxManager) {
      throw new Error('SandboxManager is not available — cannot route to container');
    }

    // Check if already running with a chatUrl
    let sandbox = this.sandboxManager.getContainerByInstance(instanceId);
    if (sandbox?.chatUrl) return sandbox.chatUrl;

    // Not running — try starting the instance (spawns container + readiness poll)
    await this.startInstance(instanceId);

    sandbox = this.sandboxManager.getContainerByInstance(instanceId);
    if (sandbox?.chatUrl) return sandbox.chatUrl;

    throw new Error(
      `Agent instance ${instanceId} started but chat URL is not available — ` +
        'container may not have acquired a network IP'
    );
  }

  /**
   * Register a TTL for an ephemeral agent instance.
   * The cleanup job will kill the container if the deadline passes.
   */
  public registerEphemeralTTL(instanceId: string, ttlMinutes: number): void {
    if (this.cleanupService) {
      this.cleanupService.registerEphemeralTTL(instanceId, ttlMinutes);
    }
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

  /**
   * Reconcile tasks on startup: mark 'running' tasks with no active container as failed/orphaned.
   */
  public async reconcileTasks(): Promise<void> {
    const activeInstanceIds = this.sandboxManager
      ? await this.sandboxManager.getActiveInstanceIds()
      : [];

    let queryText = `
       UPDATE task_queue
       SET status = 'failed',
           exit_reason = 'orphaned',
           error = 'Orphaned by system restart',
           completed_at = now()
       WHERE status = 'running'`;

    const params: string[] = [];
    if (activeInstanceIds.length > 0) {
      queryText += ` AND agent_instance_id NOT IN (${activeInstanceIds.map((_, i) => `$${i + 1}`).join(', ')})`;
      params.push(...activeInstanceIds);
    }

    const result = await query(queryText, params);
    const count = result.rowCount ?? 0;
    if (count > 0) {
      logger.info(
        `Reconciled ${count} orphaned tasks on startup (ignored ${activeInstanceIds.length} active instances)`
      );
    }
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
