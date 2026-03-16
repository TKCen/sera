import path from 'path';
import { BaseAgent } from './BaseAgent.js';
import { PrimaryAgent } from './PrimaryAgent.js';
import { WorkerAgent } from './WorkerAgent.js';
import type { AgentTask } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import { AgentManifestLoader } from './manifest/AgentManifestLoader.js';
import { ProviderFactory } from '../lib/llm/ProviderFactory.js';

export class Orchestrator {
  private agents: Map<string, BaseAgent> = new Map();
  private manifests: Map<string, AgentManifest> = new Map();
  private tasks: AgentTask[] = [];
  private primaryAgentName: string | undefined;

  /**
   * Load agents from AGENT.yaml manifests in a directory.
   * The first agent loaded becomes the primary agent (used for chat routing).
   */
  loadAgentsFromManifests(dirPath: string): void {
    const loadedManifests = AgentManifestLoader.loadAllManifests(dirPath);

    for (const manifest of loadedManifests) {
      const provider = ProviderFactory.createFromManifest(manifest);
      const agent = new WorkerAgent(manifest, provider);

      this.agents.set(manifest.metadata.name, agent);
      this.manifests.set(manifest.metadata.name, manifest);

      // First agent alphabetically or we can designate primary by convention
      if (!this.primaryAgentName) {
        this.primaryAgentName = manifest.metadata.name;
      }
    }

    console.log(`[Orchestrator] Loaded ${loadedManifests.length} agents from manifests`);

    if (this.primaryAgentName) {
      console.log(`[Orchestrator] Primary agent: ${this.primaryAgentName}`);
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

  updateLlmProvider(llmProvider: LLMProvider) {
    for (const agent of this.agents.values()) {
      agent.updateLlmProvider(llmProvider);
    }
  }

  async executeTask(description: string) {
    console.log(`[Orchestrator] Starting task: ${description}`);

    const primaryAgent = this.getPrimaryAgent();
    if (!primaryAgent) throw new Error('No primary agent configured');

    const response = await primaryAgent.process(description);

    if (response.action) {
      console.log(`[Orchestrator] Agent requested tool: ${response.action.tool}`);
      // Future: execute tool via SkillRegistry / MCPRegistry
    }

    if (response.delegation) {
      console.log(`[Orchestrator] Delegating to ${response.delegation.agentRole}`);
      const worker = this.agents.get(response.delegation.agentRole);
      if (worker) {
        const workerResponse = await worker.process(response.delegation.task);
        return workerResponse.finalAnswer;
      } else {
        throw new Error(`Agent "${response.delegation.agentRole}" not found`);
      }
    }

    return response.finalAnswer || "No answer provided.";
  }
}
