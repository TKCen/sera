import path from 'path';
import { v4 as uuidv4 } from 'uuid';
import type { AgentManifest } from './manifest/types.js';
import { AgentManifestLoader } from './manifest/AgentManifestLoader.js';
import { ProviderFactory } from '../lib/llm/ProviderFactory.js';
import { WorkerAgent } from './WorkerAgent.js';
import type { BaseAgent } from './BaseAgent.js';
import type { AgentInstance } from './types.js';
import { query, pool } from '../lib/database.js';
import { MemoryManager } from '../memory/manager.js';
import { CoreMemoryService } from '../memory/CoreMemoryService.js';
import type { LlmRouter } from '../llm/LlmRouter.js';

export class AgentFactory {
  /**
   * Create a BaseAgent implementation from a manifest and instance ID.
   *
   * When `router` is supplied, the agent's LLM provider is backed by
   * LlmRouter → pi-mono (new in-process gateway) instead of the legacy
   * OpenAIProvider chain.
   */
  static createAgent(
    manifest: AgentManifest,
    agentInstanceId?: string,
    intercom?: import('../intercom/IntercomService.js').IntercomService,
    router?: LlmRouter
  ): BaseAgent {
    const provider = ProviderFactory.createFromManifest(manifest, router);
    const memOpts: { circleId?: string; agentId?: string } = {};
    if (manifest.metadata.circle) memOpts.circleId = manifest.metadata.circle;
    if (agentInstanceId) memOpts.agentId = agentInstanceId;
    const memoryManager = new MemoryManager(memOpts);
    return new WorkerAgent(manifest, provider, intercom, agentInstanceId, memoryManager);
  }

  /**
   * Create a new persistent agent instance in the database.
   */
  static async createInstance(
    templateName: string,
    name: string,
    workspacePath: string,
    circleId?: string
  ): Promise<AgentInstance> {
    const id = uuidv4();
    const now = new Date().toISOString();

    // Sanitize name for filesystem usage
    const sanitizedName = name.toLowerCase().replace(/[^a-z0-9]/g, '-');
    const finalWorkspacePath = workspacePath || path.join('/workspaces', sanitizedName);

    await query(
      `INSERT INTO agent_instances (id, template_name, name, workspace_path, status, created_at, updated_at, circle_id)
       VALUES ($1, $2, $3, $4, 'active', $5, $5, $6)`,
      [id, templateName, name, finalWorkspacePath, now, circleId]
    );

    // Epic 08: Initialize core memory blocks
    await CoreMemoryService.getInstance(pool).initializeDefaultBlocks(id);

    return {
      id,
      template_ref: templateName,
      name,
      workspace_path: finalWorkspacePath,
      status: 'active',
      created_at: now,
      updated_at: now,
      circle_id: circleId || null,
    };
  }

  /**
   * Load an agent instance from the database.
   */
  static async getInstance(id: string): Promise<AgentInstance | null> {
    const result = await query(
      `SELECT id, template_name, name, workspace_path, container_id, status, created_at, updated_at, circle_id, lifecycle_mode, parent_instance_id, overrides, resolved_config, resolved_capabilities, sandbox_boundary, template_ref, display_name
       FROM agent_instances WHERE id = $1`,
      [id]
    );

    if (result.rows.length === 0) return null;
    const row = result.rows[0];

    return {
      id: row.id,
      template_ref: row.template_ref ?? row.template_name,
      name: row.name,
      display_name: row.display_name,
      workspace_path: row.workspace_path,
      container_id: row.container_id,
      status: row.status as AgentInstance['status'],
      created_at: row.created_at,
      updated_at: row.updated_at,
      circle_id: row.circle_id,
      lifecycle_mode: row.lifecycle_mode,
      parent_instance_id: row.parent_instance_id,
      overrides: row.overrides,
      resolved_config: row.resolved_config,
      resolved_capabilities: row.resolved_capabilities,
    };
  }

  /**
   * Update the container ID for an agent instance.
   */
  static async updateContainerId(id: string, containerId: string | null): Promise<void> {
    await query(`UPDATE agent_instances SET container_id = $1, updated_at = NOW() WHERE id = $2`, [
      containerId,
      id,
    ]);
  }

  /**
   * List all agent instances, optionally filtered by template.
   */
  static async listInstances(templateName?: string): Promise<AgentInstance[]> {
    let result;
    if (templateName) {
      result = await query(
        `SELECT * FROM agent_instances WHERE template_name = $1 ORDER BY created_at DESC`,
        [templateName]
      );
    } else {
      result = await query(`SELECT * FROM agent_instances ORDER BY created_at DESC`);
    }

    return result.rows.map((row) => ({
      id: row.id,
      template_ref: row.template_name,
      name: row.name,
      workspace_path: row.workspace_path,
      container_id: row.container_id,
      status: row.status as AgentInstance['status'],
      created_at: row.created_at,
      updated_at: row.updated_at,
    }));
  }

  /**
   * Delete an agent instance from the database.
   */
  static async deleteInstance(id: string): Promise<void> {
    await query(`DELETE FROM agent_instances WHERE id = $1`, [id]);
  }

  // ── Legacy / Template Loading ─────────────────────────────────────────────

  /**
   * Load all manifests from a directory.
   */
  static loadTemplates(dirPath: string): Map<string, AgentManifest> {
    const manifests = AgentManifestLoader.loadAllManifests(dirPath);
    return new Map(manifests.map((m) => [m.metadata.name, m]));
  }
}
