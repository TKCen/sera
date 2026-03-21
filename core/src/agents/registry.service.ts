import type { Pool } from 'pg';
import type { AgentTemplate, NamedList, CapabilityPolicy, SandboxBoundary } from './schemas.js';
import type { AgentInstance } from './types.js';
import { ScheduleService } from '../services/ScheduleService.js';

import { Logger } from '../lib/logger.js';

const logger = new Logger('AgentRegistry');

export class AgentRegistry {
  constructor(private pool: Pool) {}

  // ── Agent Templates ──────────────────────────────────────────────────────

  async upsertTemplate(template: AgentTemplate) {
    const { name, displayName, builtin, category } = template.metadata;
    const query = `
      INSERT INTO agent_templates (name, display_name, builtin, category, spec, updated_at)
      VALUES ($1, $2, $3, $4, $5, NOW())
      ON CONFLICT (name) DO UPDATE SET
        display_name = EXCLUDED.display_name,
        builtin = EXCLUDED.builtin,
        category = EXCLUDED.category,
        spec = EXCLUDED.spec,
        updated_at = NOW()
      RETURNING *;
    `;
    const res = await this.pool.query(query, [name, displayName, builtin, category, template.spec]);
    return res.rows[0];
  }

  async getTemplate(name: string) {
    const res = await this.pool.query('SELECT * FROM agent_templates WHERE name = $1', [name]);
    return res.rows[0];
  }

  async listTemplates() {
    const res = await this.pool.query('SELECT * FROM agent_templates ORDER BY name ASC');
    return res.rows;
  }

  async updateTemplate(name: string, template: AgentTemplate) {
    const existing = await this.getTemplate(name);
    if (!existing) throw new Error(`Template ${name} not found`);
    if (existing.builtin) throw new Error(`Cannot update builtin template ${name}`);

    const { displayName, category } = template.metadata;
    const query = `
      UPDATE agent_templates
      SET display_name = $2, category = $3, spec = $4, updated_at = NOW()
      WHERE name = $1
      RETURNING *;
    `;
    const res = await this.pool.query(query, [name, displayName, category, template.spec]);
    return res.rows[0];
  }

  async deleteTemplate(name: string) {
    const existing = await this.getTemplate(name);
    if (!existing) throw new Error(`Template ${name} not found`);
    if (existing.builtin) throw new Error(`Cannot delete builtin template ${name}`);

    const instances = await this.listInstances();
    const referenced = instances.some((i) => i.template_ref === name);
    if (referenced) {
      throw new Error(`Template ${name} is referenced by active instances`);
    }

    const res = await this.pool.query('DELETE FROM agent_templates WHERE name = $1 RETURNING *', [
      name,
    ]);
    return res.rows[0];
  }

  // ── Agent Instances ──────────────────────────────────────────────────────

  async createInstance(data: {
    name: string;
    displayName?: string;
    templateRef: string;
    workspacePath?: string;
    circle?: string;
    overrides?: Record<string, unknown>;
    lifecycleMode?: 'persistent' | 'ephemeral';
    parentInstanceId?: string;
  }) {
    const id = crypto.randomUUID();
    // Derive workspace path if not provided (mirrors AgentFactory convention)
    const workspacePath =
      data.workspacePath ?? `/workspaces/${data.name.toLowerCase().replace(/[^a-z0-9]/g, '-')}`;
    const query = `
      INSERT INTO agent_instances (
        id, name, display_name, template_name, template_ref, workspace_path,
        circle, lifecycle_mode, parent_instance_id, overrides, status
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'created')
      RETURNING *;
    `;
    const res = await this.pool.query(query, [
      id,
      data.name,
      data.displayName,
      data.templateRef,   // template_name: legacy NOT NULL column, kept for backward compat with AgentFactory queries
      data.templateRef,   // template_ref: canonical column used by registry and orchestrator
      workspacePath,
      data.circle,
      data.lifecycleMode ?? 'persistent',
      data.parentInstanceId,
      data.overrides ?? {},
    ]);
    const instance = res.rows[0];

    // Story 11.2: Import manifest schedules
    await this.syncManifestSchedules(instance.id, data.templateRef);

    return instance;
  }

  /**
   * Syncs schedules declared in the agent manifest with the schedules table.
   * Story 11.2
   */
  private async syncManifestSchedules(instanceId: string, templateRef: string) {
    const template = await this.getTemplate(templateRef);
    if (!template || !template.spec?.schedules) return;

    const scheduleService = ScheduleService.getInstance();
    const manifestSchedules = template.spec.schedules;

    for (const s of manifestSchedules) {
      await scheduleService
        .createSchedule({
          agent_instance_id: instanceId,
          agent_name: templateRef,
          name: s.name,
          description: s.description,
          type: s.type,
          expression: s.expression,
          task: s.task,
          source: 'manifest',
        })
        .catch((err) => {
          // Ignore duplicates
          if (!err.message.includes('unique constraint')) {
            logger.error(`Failed to sync manifest schedule ${s.name}:`, err);
          }
        });
    }
  }

  async getInstance(id: string): Promise<AgentInstance | null> {
    const res = await this.pool.query('SELECT * FROM agent_instances WHERE id = $1', [id]);
    return res.rows[0] || null;
  }

  async getInstanceByName(name: string): Promise<AgentInstance | null> {
    const res = await this.pool.query('SELECT * FROM agent_instances WHERE name = $1', [name]);
    return res.rows[0] || null;
  }

  async listInstances(
    filters: { circle?: string; status?: string } = {}
  ): Promise<AgentInstance[]> {
    let queryText = 'SELECT * FROM agent_instances';
    const params: unknown[] = [];
    const wheres: string[] = [];

    if (filters.circle) {
      params.push(filters.circle);
      wheres.push(`circle = $${params.length}`);
    }
    if (filters.status) {
      params.push(filters.status);
      wheres.push(`status = $${params.length}`);
    }

    if (wheres.length > 0) queryText += ' WHERE ' + wheres.join(' AND ');
    queryText += ' ORDER BY created_at DESC';
    const res = await this.pool.query(queryText, params);
    return res.rows;
  }

  async updateLastHeartbeat(id: string) {
    await this.pool.query(
      'UPDATE agent_instances SET last_heartbeat_at = NOW(), updated_at = NOW() WHERE id = $1',
      [id]
    );
  }

  async updateInstanceStatus(id: string, status: string, containerId?: string) {
    const query = `
      UPDATE agent_instances
      SET status = $2, container_id = COALESCE($3, container_id), updated_at = NOW()
      WHERE id = $1
      RETURNING *;
    `;
    const res = await this.pool.query(query, [id, status, containerId]);
    return res.rows[0];
  }

  async updateInstanceConfig(
    id: string,
    overrides: unknown,
    resolvedConfig?: unknown,
    resolvedCapabilities?: unknown
  ) {
    const queryText = `
      UPDATE agent_instances
      SET overrides = $2, resolved_config = $3, resolved_capabilities = $4, updated_at = NOW()
      WHERE id = $1
      RETURNING *;
    `;
    const res = await this.pool.query(queryText, [
      id,
      overrides,
      resolvedConfig,
      resolvedCapabilities,
    ]);
    return res.rows[0];
  }

  async deleteInstance(id: string) {
    const res = await this.pool.query('DELETE FROM agent_instances WHERE id = $1 RETURNING *', [
      id,
    ]);
    return res.rows[0];
  }

  // ── Subagents & Lineage (Story 3.8, 3.11) ────────────────────────────────

  /**
   * List all direct and indirect subagents of a given instance.
   * Returns instances in order from parent to leaf.
   */
  async listSubagents(parentInstanceId: string): Promise<unknown[]> {
    // Recursive CTE traverses the full subagent tree
    const queryText = `
      WITH RECURSIVE subtree AS (
        SELECT *, 0 AS lineage_depth
        FROM agent_instances
        WHERE parent_instance_id = $1
        UNION ALL
        SELECT ai.*, subtree.lineage_depth + 1
        FROM agent_instances ai
        INNER JOIN subtree ON ai.parent_instance_id = subtree.id
      )
      SELECT * FROM subtree ORDER BY lineage_depth, created_at;
    `;
    const res = await this.pool.query(queryText, [parentInstanceId]);
    return res.rows;
  }

  /**
   * Calculate the lineage depth of an instance by traversing parent_instance_id.
   * Depth 0 = operator-spawned (no parent).
   * Story 3.11
   */
  async getLineageDepth(instanceId: string): Promise<number> {
    const query = `
      WITH RECURSIVE lineage AS (
        SELECT id, parent_instance_id, 0 AS depth
        FROM agent_instances
        WHERE id = $1
        UNION ALL
        SELECT ai.id, ai.parent_instance_id, l.depth + 1
        FROM agent_instances ai
        INNER JOIN lineage l ON ai.id = l.parent_instance_id
      )
      SELECT MAX(depth) AS depth FROM lineage;
    `;
    const res = await this.pool.query(query, [instanceId]);
    return (res.rows[0]?.depth as number | null) ?? 0;
  }

  // ── Resources (NamedLists, Policies, Boundaries) ──────────────────────────

  async upsertNamedList(list: NamedList, source: string = 'file') {
    const { name, type, alwaysEnforced } = list.metadata;
    const query = `
      INSERT INTO named_lists (name, type, source, entries, always_enforced, updated_at)
      VALUES ($1, $2, $3, $4, $5, NOW())
      ON CONFLICT (name) DO UPDATE SET
        type = EXCLUDED.type,
        source = EXCLUDED.source,
        entries = EXCLUDED.entries,
        always_enforced = EXCLUDED.always_enforced,
        updated_at = NOW()
      RETURNING *;
    `;
    const res = await this.pool.query(query, [
      name,
      type,
      source,
      JSON.stringify(list.entries),
      alwaysEnforced ?? false,
    ]);
    return res.rows[0];
  }

  async upsertCapabilityPolicy(policy: CapabilityPolicy, source: string = 'file') {
    const { name } = policy.metadata;
    const query = `
      INSERT INTO capability_policies (name, source, capabilities, updated_at)
      VALUES ($1, $2, $3, NOW())
      ON CONFLICT (name) DO UPDATE SET
        source = EXCLUDED.source,
        capabilities = EXCLUDED.capabilities,
        updated_at = NOW()
      RETURNING *;
    `;
    const res = await this.pool.query(query, [name, source, policy.capabilities]);
    return res.rows[0];
  }

  async upsertSandboxBoundary(boundary: SandboxBoundary, source: string = 'file') {
    const { name } = boundary.metadata;
    const query = `
      INSERT INTO sandbox_boundaries (name, source, linux, capabilities, updated_at)
      VALUES ($1, $2, $3, $4, NOW())
      ON CONFLICT (name) DO UPDATE SET
        source = EXCLUDED.source,
        linux = EXCLUDED.linux,
        capabilities = EXCLUDED.capabilities,
        updated_at = NOW()
      RETURNING *;
    `;
    const res = await this.pool.query(query, [name, source, boundary.linux, boundary.capabilities]);
    return res.rows[0];
  }

  async getNamedList(name: string): Promise<NamedList | null> {
    const res = await this.pool.query('SELECT * FROM named_lists WHERE name = $1', [name]);
    if (!res.rows[0]) return null;
    const row = res.rows[0];
    return {
      apiVersion: 'sera/v1',
      kind: 'NamedList',
      metadata: {
        name: row.name,
        type: row.type,
        description: row.description,
        alwaysEnforced: row.always_enforced,
      },
      entries: row.entries,
    };
  }

  async getCapabilityPolicy(name: string) {
    const res = await this.pool.query('SELECT * FROM capability_policies WHERE name = $1', [name]);
    return res.rows[0];
  }

  async getSandboxBoundary(name: string) {
    const res = await this.pool.query('SELECT * FROM sandbox_boundaries WHERE name = $1', [name]);
    return res.rows[0];
  }

  async listAlwaysEnforcedNamedLists() {
    const res = await this.pool.query('SELECT * FROM named_lists WHERE always_enforced = true');
    return res.rows;
  }

  // ── Capability Grants (Story 3.9, 3.10) ──────────────────────────────────

  async createCapabilityGrant(data: {
    agentInstanceId: string;
    dimension: string;
    value: string;
    grantType: 'one-time' | 'session' | 'persistent';
    grantedBy?: string;
    grantedByEmail?: string;
    grantedByName?: string;
    expiresAt?: string;
  }) {
    const query = `
      INSERT INTO capability_grants
        (agent_instance_id, dimension, value, grant_type, granted_by, granted_by_email, granted_by_name, expires_at)
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
      RETURNING *;
    `;
    const res = await this.pool.query(query, [
      data.agentInstanceId,
      data.dimension,
      data.value,
      data.grantType,
      data.grantedBy ?? null,
      data.grantedByEmail ?? null,
      data.grantedByName ?? null,
      data.expiresAt ?? null,
    ]);
    return res.rows[0];
  }

  async listCapabilityGrants(agentInstanceId: string, includeRevoked = false): Promise<unknown[]> {
    let query = `
      SELECT
        id, agent_instance_id, dimension, value, grant_type,
        granted_by, granted_by_email, granted_by_name,
        expires_at, revoked_at, created_at,
        CASE
          WHEN granted_by IS NOT NULL THEN
            json_build_object(
              'sub', granted_by,
              'email', granted_by_email,
              'name', granted_by_name
            )
          ELSE NULL
        END AS "grantedBy",
        created_at AS "grantedAt"
      FROM capability_grants
      WHERE agent_instance_id = $1
        AND (expires_at IS NULL OR expires_at > NOW())
    `;
    if (!includeRevoked) {
      query += ' AND revoked_at IS NULL';
    }
    query += ' ORDER BY created_at DESC';
    const res = await this.pool.query(query, [agentInstanceId]);
    return res.rows;
  }

  async revokeCapabilityGrant(grantId: string) {
    const res = await this.pool.query(
      'UPDATE capability_grants SET revoked_at = NOW() WHERE id = $1 AND revoked_at IS NULL RETURNING *',
      [grantId]
    );
    return res.rows[0];
  }

  // ── Workspace Usage (Story 3.12) ─────────────────────────────────────────

  async updateWorkspaceUsage(instanceId: string, usedGB: number) {
    await this.pool.query(
      'UPDATE agent_instances SET workspace_used_gb = $2, updated_at = NOW() WHERE id = $1',
      [instanceId, usedGB]
    );
  }
}
