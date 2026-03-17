import fs from 'fs';
import path from 'path';
import { BaseAgent } from './BaseAgent.js';
import { AgentFactory } from './AgentFactory.js';
import { ProcessManager } from './process/ProcessManager.js';
import type { ProcessType, ProcessTask, ProcessRunResult } from './process/types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import { Logger } from '../lib/logger.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import type { ToolExecutor } from '../tools/ToolExecutor.js';
import type { IdentityService } from '../auth/IdentityService.js';

const logger = new Logger('Orchestrator');

export class Orchestrator {
  private agents: Map<string, BaseAgent> = new Map();
  private manifests: Map<string, AgentManifest> = new Map();
  private primaryAgentName: string | undefined;
  private processManager: ProcessManager = new ProcessManager();
  private intercom: IntercomService | undefined;
  private toolExecutor: ToolExecutor | undefined;
  private sandboxManager: import('../sandbox/SandboxManager.js').SandboxManager | undefined;
  private identityService: IdentityService | undefined;

  /** Last heartbeat timestamp per agent instance ID. */
  private heartbeats: Map<string, Date> = new Map();

  /** Active file watcher (if any). */
  private watcher: fs.FSWatcher | undefined;
  private agentsDir: string | undefined;

  /**
   * Set the IntercomService and propagate to all loaded agents.
   */
  public setIntercom(intercom: IntercomService): void {
    this.intercom = intercom;
    for (const agent of this.agents.values()) {
      agent.setIntercom(intercom);
    }
  }

  /**
   * Set the ToolExecutor and propagate to all loaded agents.
   */
  public setToolExecutor(toolExecutor: ToolExecutor): void {
    this.toolExecutor = toolExecutor;
    for (const agent of this.agents.values()) {
      agent.setToolExecutor(toolExecutor);
    }
  }

  /** Attach a SandboxManager after construction. */
  public setSandboxManager(sandboxManager: import('../sandbox/SandboxManager.js').SandboxManager): void {
    this.sandboxManager = sandboxManager;
  }

  /** Attach an IdentityService for JWT issuance. */
  public setIdentityService(identityService: IdentityService): void {
    this.identityService = identityService;
  }

  /** Get the tool executor. */
  public getToolExecutor(): ToolExecutor | undefined {
    return this.toolExecutor;
  }

  /**
   * Load agent templates from AGENT.yaml manifests in a directory.
   */
  loadTemplates(dirPath: string): void {
    this.agentsDir = dirPath;
    this.manifests = AgentFactory.loadTemplates(dirPath);

    // Set primary to alphabetically first template
    const sorted = Array.from(this.manifests.keys()).sort();
    if (sorted.length > 0) {
      this.primaryAgentName = sorted[0];
    }

    logger.info(`Loaded ${this.manifests.size} agent templates`);
  }

  /**
   * Instantiate an agent from a template and start it.
   */
  async startInstance(instanceId: string): Promise<BaseAgent> {
    const instance = await AgentFactory.getInstance(instanceId);
    if (!instance) throw new Error(`Agent instance "${instanceId}" not found`);

    const manifest = this.manifests.get(instance.templateName);
    if (!manifest) throw new Error(`Template "${instance.templateName}" not found`);

    const agent = AgentFactory.createAgent(manifest, instance.id, this.intercom);
    if (this.toolExecutor) agent.setToolExecutor(this.toolExecutor);
    
    // ── Spawn Container if necessary ────────────────────────────────────────
    if (this.sandboxManager) {
      try {
        // Issue a JWT for the agent container
        let identityToken: string | undefined;
        if (this.identityService) {
          identityToken = this.identityService.signToken({
            agentId: instance.id,
            circleId: manifest.metadata.circle,
            capabilities: manifest.tools?.allowed ?? [],
          });
          logger.info(`Issued JWT for agent ${instance.name} (${instance.id})`);
        }

        const sandbox = await this.sandboxManager.spawn(manifest, {
          type: 'agent',
          agentName: manifest.metadata.name,
          image: 'sera-agent-worker:latest',
          hostWorkspacePath: instance.workspacePath,
          env: {
            AGENT_NAME: instance.name,
            AGENT_INSTANCE_ID: instance.id,
            SERA_CORE_URL: process.env.SERA_API_URL || 'http://sera-core:3001',
            ...(identityToken ? { SERA_IDENTITY_TOKEN: identityToken } : {}),
            CENTRIFUGO_API_URL: process.env.CENTRIFUGO_API_URL || 'http://centrifugo:8000/api',
            CENTRIFUGO_API_KEY: process.env.CENTRIFUGO_API_KEY || 'sera-api-key',
          }
        });
        
        agent.setContainerId(sandbox.containerId);
        await AgentFactory.updateContainerId(instance.id, sandbox.containerId);
        logger.info(`Spawned container ${sandbox.containerId} for agent ${instance.name}`);
      } catch (err) {
        logger.error(`Failed to spawn container for agent ${instance.name}:`, err);
        // Fallback or rethrow based on policy
      }
    } else if (instance.containerId) {
      // If containerId is already set in DB, restore it
      agent.setContainerId(instance.containerId);
    }

    this.agents.set(instance.id, agent);
    logger.info(`Started agent instance: ${instance.name} (${instance.id})`);
    return agent;
  }

  /**
   * Stop an agent instance, removing it from memory and cleaning up its container.
   */
  async stopInstance(instanceId: string): Promise<void> {
    const agent = this.agents.get(instanceId);
    if (!agent) {
      logger.warn(`Agent instance "${instanceId}" not running in memory.`);
      // Even if not in memory, we might need to cleanup the container if it's in DB
      const instance = await AgentFactory.getInstance(instanceId);
      if (instance?.containerId && this.sandboxManager) {
        const manifest = this.manifests.get(instance.templateName);
        if (manifest) {
          try {
            await this.sandboxManager.remove(manifest, instance.containerId);
            logger.info(`Cleaned up orphaned container ${instance.containerId} for ${instanceId}`);
          } catch (err) {
            logger.error(`Failed to cleanup orphaned container ${instance.containerId}:`, err);
          }
        }
      }
      return;
    }

    // Stop container
    if (this.sandboxManager && agent.containerId) {
      try {
        await this.sandboxManager.remove(agent.getManifest(), agent.containerId);
        logger.info(`Stopped container ${agent.containerId} for agent ${instanceId}`);
      } catch (err) {
        logger.error(`Failed to stop container ${agent.containerId} for agent ${instanceId}:`, err);
      }
    }

    this.agents.delete(instanceId);
    logger.info(`Stopped agent instance: ${instanceId}`);
  }

  /**
   * Re-scan the templates directory.
   */
  reloadTemplates(dirPath?: string): { count: number } {
    const dir = dirPath ?? this.agentsDir;
    if (!dir) throw new Error('No agents directory configured');

    this.manifests = AgentFactory.loadTemplates(dir);
    logger.info(`Reloaded ${this.manifests.size} agent templates`);
    return { count: this.manifests.size };
  }

  /**
   * Get all loaded agent templates.
   */
  getAllTemplates(): AgentManifest[] {
    return Array.from(this.manifests.values());
  }

  /**
   * Watch the agents directory for .agent.yaml changes and auto-reload.
   */
  watchAgentsDirectory(dirPath?: string): void {
    const dir = dirPath ?? this.agentsDir;
    if (!dir) {
      logger.warn('No agents directory to watch');
      return;
    }

    if (this.watcher) {
      this.watcher.close();
    }

    let debounceTimer: ReturnType<typeof setTimeout> | undefined;

    this.watcher = fs.watch(dir, (eventType, filename) => {
      if (!filename?.endsWith('.agent.yaml')) return;

      // Debounce rapid changes (e.g., editor save)
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        logger.info(`Detected change in ${filename}, reloading templates...`);
        try {
          this.reloadTemplates(dir);
        } catch (err) {
          logger.error(`Failed to reload templates:`, err);
        }
      }, 500);
    });

    logger.info(`Watching for agent changes in ${dir}`);
  }

  /**
   * Stop the file watcher.
   */
  stopWatching(): void {
    if (this.watcher) {
      this.watcher.close();
      this.watcher = undefined;
      logger.info('Stopped watching agents directory');
    }
  }

  /**
   * Designate which agent acts as the primary conversational agent.
   */
  setPrimaryAgent(name: string): void {
    if (!this.agents.has(name)) {
      throw new Error(`Agent "${name}" not registered`);
    }
    this.primaryAgentName = name;
  }

  registerAgent(agent: BaseAgent) {
    this.agents.set(agent.role, agent);
  }

  getAgent(name: string): BaseAgent | undefined {
    return this.agents.get(name);
  }

  getManifest(name: string): AgentManifest | undefined {
    return this.manifests.get(name);
  }

  getAllManifests(): AgentManifest[] {
    return Array.from(this.manifests.values());
  }

  getPrimaryAgent(): BaseAgent | undefined {
    if (this.primaryAgentName) {
      return this.agents.get(this.primaryAgentName);
    }
    return undefined;
  }

  /**
   * Returns basic info about all loaded agents.
   */
  listAgents(): Array<{
    name: string;
    displayName: string;
    role: string;
    tier: number;
    circle: string;
    icon: string;
  }> {
    return Array.from(this.manifests.values()).map(m => ({
      name: m.metadata.name,
      displayName: m.metadata.displayName,
      role: m.identity.role,
      tier: m.metadata.tier,
      circle: m.metadata.circle,
      icon: m.metadata.icon,
    }));
  }

  /**
   * Get detailed info for a single agent.
   */
  getAgentInfo(name: string): {
    name: string;
    displayName: string;
    role: string;
    tier: number;
    circle: string;
    icon: string;
    manifest: AgentManifest;
  } | undefined {
    const manifest = this.manifests.get(name);
    if (!manifest) return undefined;
    return {
      name: manifest.metadata.name,
      displayName: manifest.metadata.displayName,
      role: manifest.identity.role,
      tier: manifest.metadata.tier,
      circle: manifest.metadata.circle,
      icon: manifest.metadata.icon,
      manifest,
    };
  }

  updateLlmProvider(llmProvider: LLMProvider) {
    for (const agent of this.agents.values()) {
      agent.updateLlmProvider(llmProvider);
    }
  }

  // ── Heartbeat Lifecycle ─────────────────────────────────────────────────────

  /** Record a heartbeat for an agent instance. */
  recordHeartbeat(instanceId: string): void {
    this.heartbeats.set(instanceId, new Date());
  }

  /** Get the last heartbeat time for an agent instance. */
  getLastHeartbeat(instanceId: string): Date | undefined {
    return this.heartbeats.get(instanceId);
  }

  /**
   * Get instances that haven't sent a heartbeat within the timeout.
   * Only considers instances that have sent at least one heartbeat.
   */
  getUnhealthyInstances(timeoutMs: number = 30_000): Array<{ instanceId: string; lastSeen: Date }> {
    const now = Date.now();
    const unhealthy: Array<{ instanceId: string; lastSeen: Date }> = [];

    for (const [instanceId, lastSeen] of this.heartbeats.entries()) {
      if (now - lastSeen.getTime() > timeoutMs) {
        unhealthy.push({ instanceId, lastSeen });
      }
    }

    return unhealthy;
  }

  // ── Process-Managed Execution ──────────────────────────────────────────────

  /**
   * Execute a single task using the primary agent (backward-compatible).
   * Uses the ProcessManager with sequential strategy internally.
   */
  async executeTask(description: string) {
    logger.info(`Starting task: ${description}`);

    const primaryAgent = this.getPrimaryAgent();
    if (!primaryAgent) throw new Error('No primary agent configured');

    const result = await this.processManager.runSingle(description, primaryAgent);
    return result.finalOutput || 'No answer provided.';
  }

  /**
   * Execute multiple tasks using a specified process pattern.
   */
  async executeWithProcess(
    type: ProcessType,
    tasks: ProcessTask[],
    managerAgentName?: string,
  ): Promise<ProcessRunResult> {
    const managerAgent = managerAgentName
      ? this.agents.get(managerAgentName)
      : undefined;

    return this.processManager.run(type, tasks, this.agents, managerAgent);
  }
}

