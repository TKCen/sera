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

const logger = new Logger('Orchestrator');

export class Orchestrator {
  private agents: Map<string, BaseAgent> = new Map();
  private manifests: Map<string, AgentManifest> = new Map();
  private primaryAgentName: string | undefined;
  private processManager: ProcessManager = new ProcessManager();
  private intercom: IntercomService | undefined;

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
   * Load agents from AGENT.yaml manifests in a directory.
   * The first agent loaded becomes the primary agent (used for chat routing).
   */
  loadAgentsFromManifests(dirPath: string): void {
    this.agentsDir = dirPath;
    const { agents, manifests } = AgentFactory.createAllFromDirectory(dirPath);

    this.agents = agents;
    this.manifests = manifests;

    // Set primary to alphabetically first agent
    const sorted = Array.from(manifests.keys()).sort();
    if (sorted.length > 0) {
      this.primaryAgentName = sorted[0];
    }

    logger.info(`Loaded ${manifests.size} agents from manifests`);
    if (this.primaryAgentName) {
      logger.info(`Primary agent: ${this.primaryAgentName}`);
    }
  }

  /**
   * Re-scan the agents directory and reload manifests.
   * Adds new agents, removes deleted ones, and updates changed ones.
   */
  reloadAgents(dirPath?: string): {
    added: string[];
    removed: string[];
    updated: string[];
  } {
    const dir = dirPath ?? this.agentsDir;
    if (!dir) {
      throw new Error('No agents directory configured. Call loadAgentsFromManifests first.');
    }

    const diff = AgentFactory.diffAgents(this.manifests, dir);

    // Remove deleted agents
    for (const name of diff.removed) {
      this.agents.delete(name);
      this.manifests.delete(name);
      logger.info(`Removed agent: ${name}`);
    }

    // Add new agents
    for (const manifest of diff.added) {
      const agent = AgentFactory.createAgent(manifest);
      if (this.intercom) agent.setIntercom(this.intercom);
      this.agents.set(manifest.metadata.name, agent);
      this.manifests.set(manifest.metadata.name, manifest);
      logger.info(`Added agent: ${manifest.metadata.name}`);
    }

    // Update changed agents
    for (const manifest of diff.updated) {
      const agent = AgentFactory.createAgent(manifest);
      if (this.intercom) agent.setIntercom(this.intercom);
      this.agents.set(manifest.metadata.name, agent);
      this.manifests.set(manifest.metadata.name, manifest);
      logger.info(`Updated agent: ${manifest.metadata.name}`);
    }

    // Reset primary if it was removed
    if (this.primaryAgentName && !this.agents.has(this.primaryAgentName)) {
      const sorted = Array.from(this.manifests.keys()).sort();
      this.primaryAgentName = sorted[0];
      logger.info(`Primary agent reassigned: ${this.primaryAgentName}`);
    }

    return {
      added: diff.added.map(m => m.metadata.name),
      removed: diff.removed,
      updated: diff.updated.map(m => m.metadata.name),
    };
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
        logger.info(`Detected change in ${filename}, reloading agents...`);
        try {
          this.reloadAgents(dir);
        } catch (err) {
          logger.error(`Failed to reload agents:`, err);
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
