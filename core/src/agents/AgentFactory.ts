/**
 * AgentFactory — creates agent instances from YAML manifests.
 *
 * Provides a centralized way to instantiate agents with their correct
 * LLM providers, supporting dynamic agent creation at runtime.
 */

import type { AgentManifest } from './manifest/types.js';
import { AgentManifestLoader } from './manifest/AgentManifestLoader.js';
import { ProviderFactory } from '../lib/llm/ProviderFactory.js';
import { WorkerAgent } from './WorkerAgent.js';
import type { BaseAgent } from './BaseAgent.js';

export class AgentFactory {
  /**
   * Create an agent instance from a manifest.
   */
  static createAgent(manifest: AgentManifest): BaseAgent {
    const provider = ProviderFactory.createFromManifest(manifest);
    return new WorkerAgent(manifest, provider);
  }

  /**
   * Load all manifests from a directory and create agent instances.
   * Returns both the agents map and the raw manifests.
   */
  static createAllFromDirectory(dirPath: string): {
    agents: Map<string, BaseAgent>;
    manifests: Map<string, AgentManifest>;
  } {
    const loadedManifests = AgentManifestLoader.loadAllManifests(dirPath);
    const agents = new Map<string, BaseAgent>();
    const manifests = new Map<string, AgentManifest>();

    for (const manifest of loadedManifests) {
      const agent = AgentFactory.createAgent(manifest);
      agents.set(manifest.metadata.name, agent);
      manifests.set(manifest.metadata.name, manifest);
    }

    return { agents, manifests };
  }

  /**
   * Diff the currently loaded agents against a fresh scan of a directory,
   * returning which agents were added, removed, or updated.
   */
  static diffAgents(
    current: Map<string, AgentManifest>,
    dirPath: string,
  ): {
    added: AgentManifest[];
    removed: string[];
    updated: AgentManifest[];
  } {
    const fresh = AgentManifestLoader.loadAllManifests(dirPath);
    const freshMap = new Map(fresh.map(m => [m.metadata.name, m]));

    const added: AgentManifest[] = [];
    const updated: AgentManifest[] = [];
    const removed: string[] = [];

    // Find added and updated
    for (const [name, manifest] of freshMap) {
      if (!current.has(name)) {
        added.push(manifest);
      } else {
        // Simple change detection: compare serialized manifests
        const existing = current.get(name)!;
        if (JSON.stringify(existing) !== JSON.stringify(manifest)) {
          updated.push(manifest);
        }
      }
    }

    // Find removed
    for (const name of current.keys()) {
      if (!freshMap.has(name)) {
        removed.push(name);
      }
    }

    return { added, removed, updated };
  }
}
