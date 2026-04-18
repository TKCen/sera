import type { Pool } from 'pg';
import type { AgentTemplate, NamedList, CapabilityPolicy, SandboxBoundary } from './schemas.js';
import type { AgentInstance } from './types.js';
import { ScheduleService } from '../services/ScheduleService.js';
import { CoreMemoryService } from '../memory/CoreMemoryService.js';
import { DEFAULT_HOURLY_QUOTA, DEFAULT_DAILY_QUOTA } from '../metering/MeteringService.js';

import { Logger } from '../lib/logger.js';

export interface TemplateDiffChange {
  path: string;
  type: 'added' | 'removed' | 'changed';
  oldValue?: unknown;
  newValue?: unknown;
  impact: 'info' | 'permission' | 'resource' | 'breaking';
}

export interface TemplateDiff {
  hasChanges: boolean;
  instanceId: string;
  templateName: string;
  templateUpdatedAt: string;
  instanceAppliedAt: string | null;
  changes: TemplateDiffChange[];
}

export interface PendingUpdateEntry {
  instanceId: string;
  instanceName: string;
  templateName: string;
  templateUpdatedAt: string;
}

function classifyImpact(
  path: string,
  type: 'added' | 'removed' | 'changed'
): TemplateDiffChange['impact'] {
  const permissionKeywords = [
    'capabilities',
    'tools',
    'permissions',
    'sandboxBoundary',
    'policyRef',
  ];
  const resourceKeywords = ['resources', 'memory', 'cpu', 'maxLlm', 'model', 'fallback'];
  if (permissionKeywords.some((k) => path.includes(k))) return 'permission';
  if (resourceKeywords.some((k) => path.includes(k))) return 'resource';
  if (type === 'removed') return 'breaking';
  return 'info';
}

function deepDiff(
  oldObj: unknown,
  newObj: unknown,
  path: string,
  changes: TemplateDiffChange[]
): void {
  if (oldObj === newObj) return;

  const oldIsObj = oldObj !== null && typeof oldObj === 'object' && !Array.isArray(oldObj);
  const newIsObj = newObj !== null && typeof newObj === 'object' && !Array.isArray(newObj);

  if (oldIsObj && newIsObj) {
    const oldRecord = oldObj as Record<string, unknown>;
    const newRecord = newObj as Record<string, unknown>;
    const allKeys = new Set([...Object.keys(oldRecord), ...Object.keys(newRecord)]);
    for (const key of allKeys) {
      deepDiff(oldRecord[key], newRecord[key], path ? `${path}.${key}` : key, changes);
    }
    return;
  }

  if (oldObj === undefined && newObj !== undefined) {
    changes.push({ path, type: 'added', newValue: newObj, impact: classifyImpact(path, 'added') });
    return;
  }
  if (oldObj !== undefined && newObj === undefined) {
    changes.push({
      path,
      type: 'removed',
      oldValue: oldObj,
      impact: classifyImpact(path, 'removed'),
    });
    return;
  }

  const oldSer = JSON.stringify(oldObj);
  const newSer = JSON.stringify(newObj);
  if (oldSer !== newSer) {
    changes.push({
      path,
      type: 'changed',
      oldValue: oldObj,
      newValue: newObj,
      impact: classifyImpact(path, 'changed'),
    });
  }
}

const logger = new Logger('AgentRegistry');

export class AgentRegistry {
  constructor(private pool: Pool) {}

  // ── Agent Templates ──────────────────────────────────────────────────────

  async upsertTemplate(template: AgentTemplate) {
    const { name, displayName, builtin, category } = template.metadata;
    // We check for existence first to return accurate status (added vs updated)
    const existing = await this.getTemplate(name);
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

    return {
      status: existing ? 'updated' : 'added',
      name: res.rows[0].name,
      record: res.rows[0],
    };
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
    allowedCircles?: string[];
    overrides?: Record<string, unknown>;
    lifecycleMode?: 'persistent' | 'ephemeral';
    parentInstanceId?: string;
  }) {
    // Check for duplicate instance name
    const existing = await this.pool.query('SELECT id FROM agent_instances WHERE name = $1', [
      data.name,
    ]);
    if (existing.rows.length > 0) {
      const err = new Error(`Agent instance with name "${data.name}" already exists`);
      (err as Error & { status: number }).status = 409;
      throw err;
    }

    const id = crypto.randomUUID();
    // Derive workspace path if not provided (mirrors AgentFactory convention)
    const workspacePath =
      data.workspacePath ?? `/workspaces/${data.name.toLowerCase().replace(/[^a-z0-9]/g, '-')}`;

    const template = await this.getTemplate(data.templateRef);
    const templateSpec = template?.spec ?? {};

    const query = `
      INSERT INTO agent_instances (
        id, name, display_name, template_name, template_ref, workspace_path,
        circle, allowed_circles, lifecycle_mode, parent_instance_id, overrides, status,
        resolved_config, template_applied_at
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'created', $12, NOW())
      RETURNING *;
    `;
    const res = await this.pool.query(query, [
      id,
      data.name,
      data.displayName,
      data.templateRef, // template_name: legacy NOT NULL column, kept for backward compat with AgentFactory queries
      data.templateRef, // template_ref: canonical column used by registry and orchestrator
      workspacePath,
      data.circle,
      data.allowedCircles ?? [],
      data.lifecycleMode ?? 'persistent',
      data.parentInstanceId,
      data.overrides ?? {},
      templateSpec,
    ]);
    const instance = res.rows[0];

    // Story 11.2: Import manifest schedules
    await this.syncManifestSchedules(instance.id, templateSpec, data.templateRef);

    // Sync manifest budget limits to token_quotas (syncManifestBudget merges instance overrides)
    await this.syncManifestBudget(instance.id, templateSpec);

    // Epic 08: Initialize core memory blocks
    await CoreMemoryService.getInstance(this.pool).initializeDefaultBlocks(instance.id);

    return instance;
  }

  /**
   * Syncs schedules declared in the agent template with the schedules table.
   * Uses upsert semantics: new schedules are created, existing manifest schedules
   * are updated, and stale manifest schedules (removed from template) are deleted.
   * Operator-created API schedules are never touched.
   */
  private async syncManifestSchedules(instanceId: string, spec: any, templateName: string) {
    const scheduleService = ScheduleService.getInstance();

    const manifestSchedules = (spec?.schedules ?? []) as Array<{
      name: string;
      description?: string;
      type: 'cron' | 'once';
      expression: string;
      task: string;
      status: 'active' | 'paused' | 'completed' | 'error';
      category?: string;
    }>;

    // Upsert each manifest schedule
    for (const s of manifestSchedules) {
      await scheduleService
        .upsertManifestSchedule({
          agent_instance_id: instanceId,
          agent_name: templateName,
          name: s.name,
          ...(s.description !== undefined ? { description: s.description } : {}),
          type: s.type,
          expression: s.expression,
          task: s.task,
          status: s.status,
          ...(s.category !== undefined ? { category: s.category } : {}),
        })
        .catch((err) => {
          logger.error(`Failed to sync manifest schedule ${s.name}:`, err);
        });
    }

    // Remove schedules no longer in the manifest
    const manifestNames = manifestSchedules.map((s) => s.name);
    await scheduleService.removeStaleManifestSchedules(instanceId, manifestNames);
  }

  /**
   * Syncs token budget limits from the template spec.resources to token_quotas.
   * Only writes if the template defines maxLlmTokensPerHour or maxLlmTokensPerDay.
   * Uses 0 as "unlimited" sentinel.
   */
  private async syncManifestBudget(instanceId: string, spec: any) {
    if (!spec) return;

    // Merge: instance overrides take priority over template spec
    const baseResources = spec.resources as
      | { maxLlmTokensPerHour?: number; maxLlmTokensPerDay?: number }
      | undefined;

    // Fetch instance overrides from DB (they may define budget values that override the template)
    let overrideResources:
      | { maxLlmTokensPerHour?: number; maxLlmTokensPerDay?: number }
      | undefined;
    try {
      const res = await this.pool.query(
        `SELECT overrides->'resources' AS resources FROM agent_instances WHERE id = $1`,
        [instanceId]
      );
      const row = res.rows[0];
      if (row?.resources && typeof row.resources === 'object') {
        overrideResources = row.resources as typeof overrideResources;
      }
    } catch {
      // Non-fatal — fall through to template-only resources
    }

    const resources = { ...baseResources, ...overrideResources };
    if (!resources) return;

    const hourly = resources.maxLlmTokensPerHour;
    const daily = resources.maxLlmTokensPerDay;

    // Only sync if the template defines at least one budget field
    if (hourly === undefined && daily === undefined) return;

    await this.pool
      .query(
        `INSERT INTO token_quotas (agent_id, max_tokens_per_hour, max_tokens_per_day, source, updated_at)
         VALUES ($1, $2, $3, 'manifest', NOW())
         ON CONFLICT (agent_id)
         DO UPDATE SET
           max_tokens_per_hour = EXCLUDED.max_tokens_per_hour,
           max_tokens_per_day = EXCLUDED.max_tokens_per_day,
           updated_at = NOW()
         WHERE token_quotas.source = 'manifest'`,
        [instanceId, hourly ?? DEFAULT_HOURLY_QUOTA, daily ?? DEFAULT_DAILY_QUOTA]
      )
      .catch((err) => {
        logger.error(`Failed to sync manifest budget for ${instanceId}:`, err);
      });
  }

  /**
   * Re-syncs manifest budgets for all instances, merging instance overrides with template spec.
   * Called on startup to ensure overrides are reflected in token_quotas.
   */
  async syncAllInstanceBudgets() {
    const instances = await this.listInstances();
    for (const inst of instances) {
      const template = await this.getTemplate(inst.template_ref);
      const templateSpec = template?.spec ?? {};
      await this.syncManifestBudget(inst.id, templateSpec);
    }
  }

  async getInstance(id: string): Promise<AgentInstance | null> {
    const res = await this.pool.query(
      `SELECT ai.*, COALESCE(ai.circle, c.name) AS circle
       FROM agent_instances ai
       LEFT JOIN circles c ON ai.circle_id = c.id
       WHERE ai.id = $1`,
      [id]
    );
    return res.rows[0] || null;
  }

  async getInstanceByName(name: string): Promise<AgentInstance | null> {
    const res = await this.pool.query(
      `SELECT ai.*, COALESCE(ai.circle, c.name) AS circle
       FROM agent_instances ai
       LEFT JOIN circles c ON ai.circle_id = c.id
       WHERE ai.name = $1`,
      [name]
    );
    return res.rows[0] || null;
  }

  async listInstances(
    filters: { circle?: string; status?: string } = {}
  ): Promise<AgentInstance[]> {
    let queryText = `SELECT ai.*, COALESCE(ai.circle, c.name) AS circle
       FROM agent_instances ai
       LEFT JOIN circles c ON ai.circle_id = c.id`;
    const params: unknown[] = [];
    const wheres: string[] = [];

    if (filters.circle) {
      params.push(filters.circle);
      wheres.push(`COALESCE(ai.circle, c.name) = $${params.length}`);
    }
    if (filters.status) {
      params.push(filters.status);
      wheres.push(`ai.status = $${params.length}`);
    }

    if (wheres.length > 0) queryText += ' WHERE ' + wheres.join(' AND ');
    queryText += ' ORDER BY ai.created_at DESC';
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
    let query: string;
    let params: (string | null | undefined)[];

    if (status === 'unresponsive') {
      // Guard: only mark unresponsive if no recent heartbeat arrived
      // This prevents the race where a heartbeat arrives between the stale check and this UPDATE
      const staleMs = parseInt(process.env.HEARTBEAT_STALE_MS ?? '120000', 10);
      query = `
        UPDATE agent_instances
        SET status = $2, container_id = COALESCE($3, container_id), updated_at = NOW()
        WHERE id = $1
          AND (last_heartbeat_at IS NULL OR last_heartbeat_at < NOW() - INTERVAL '1 millisecond' * $4)
        RETURNING *;
      `;
      params = [id, status, containerId ?? null, String(staleMs)];
    } else {
      query = `
        UPDATE agent_instances
        SET status = $2, container_id = COALESCE($3, container_id), updated_at = NOW()
        WHERE id = $1
        RETURNING *;
      `;
      params = [id, status, containerId ?? null];
    }

    const res = await this.pool.query(query, params);
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

  async updateInstance(id: string, fields: Record<string, unknown>) {
    const allowed = [
      'name',
      'display_name',
      'circle',
      'allowed_circles',
      'lifecycle_mode',
      'overrides',
    ];
    const setClauses: string[] = [];
    const values: unknown[] = [id];
    let idx = 2;
    for (const key of Object.keys(fields)) {
      if (allowed.includes(key)) {
        setClauses.push(`${key} = $${idx}`);
        values.push(key === 'overrides' ? JSON.stringify(fields[key]) : fields[key]);
        idx++;
      }
    }
    if (setClauses.length === 0) return this.getInstance(id);
    setClauses.push('updated_at = NOW()');
    const queryText = `UPDATE agent_instances SET ${setClauses.join(', ')} WHERE id = $1 RETURNING *`;
    const res = await this.pool.query(queryText, values);
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
    const existing = await this.getNamedList(name);
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
    return {
      status: existing ? 'updated' : 'added',
      name: res.rows[0].name,
      record: res.rows[0],
    };
  }

  async upsertCapabilityPolicy(policy: CapabilityPolicy, source: string = 'file') {
    const { name } = policy.metadata;
    const existing = await this.getCapabilityPolicy(name);
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
    return {
      status: existing ? 'updated' : 'added',
      name: res.rows[0].name,
      record: res.rows[0],
    };
  }

  async upsertSandboxBoundary(boundary: SandboxBoundary, source: string = 'file') {
    const { name } = boundary.metadata;
    const existing = await this.getSandboxBoundary(name);
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
    const res = await this.pool.query(query, [
      name,
      source,
      boundary.spec.linux,
      boundary.spec.capabilities,
    ]);
    return {
      status: existing ? 'updated' : 'added',
      name: res.rows[0].name,
      record: res.rows[0],
    };
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

  /**
   * Get active (non-expired, non-revoked) filesystem grants for an agent instance.
   * Used by Story 3.10 for persistent grant validation and bind mount assembly.
   */
  async getActiveFilesystemGrants(
    agentInstanceId: string
  ): Promise<Array<{ id: string; value: string; grant_type: string }>> {
    const res = await this.pool.query(
      `SELECT id, value, grant_type, created_at
       FROM capability_grants
       WHERE agent_instance_id = $1
         AND dimension = 'filesystem'
         AND revoked_at IS NULL
         AND (expires_at IS NULL OR expires_at > NOW())
       UNION ALL
       SELECT id, resource_value AS value, grant_type, created_at
       FROM permission_grants
       WHERE agent_instance_id = $1
         AND resource_type = 'filesystem'
         AND revoked_at IS NULL
         AND (expires_at IS NULL OR expires_at > NOW())
       ORDER BY created_at DESC`,
      [agentInstanceId]
    );
    return res.rows as Array<{ id: string; value: string; grant_type: string }>;
  }

  async revokeCapabilityGrant(grantId: string) {
    const res = await this.pool.query(
      'UPDATE capability_grants SET revoked_at = NOW() WHERE id = $1 AND revoked_at IS NULL RETURNING *',
      [grantId]
    );
    return res.rows[0];
  }

  // ── Permission Grants (ADR-004) ──────────────────────────────────────────

  async createPermissionGrant(data: {
    agentInstanceId: string;
    grantType: 'session' | 'one-time' | 'persistent';
    resourceType: string;
    resourceValue: string;
    mode?: string | undefined;
    approvedBy?: string | undefined;
    expiresAt?: string | undefined;
  }) {
    const query = `
      INSERT INTO permission_grants
        (agent_instance_id, grant_type, resource_type, resource_value, mode, approved_by, expires_at)
      VALUES ($1, $2, $3, $4, $5, $6, $7)
      RETURNING *;
    `;
    const res = await this.pool.query(query, [
      data.agentInstanceId,
      data.grantType,
      data.resourceType,
      data.resourceValue,
      data.mode ?? 'ro',
      data.approvedBy ?? null,
      data.expiresAt ?? null,
    ]);
    return res.rows[0];
  }

  async listActivePermissionGrants(agentInstanceId?: string): Promise<unknown[]> {
    let query = `
      SELECT * FROM permission_grants
      WHERE revoked_at IS NULL
        AND (expires_at IS NULL OR expires_at > NOW())
    `;
    const params: any[] = [];
    if (agentInstanceId) {
      query += ' AND agent_instance_id = $1';
      params.push(agentInstanceId);
    }
    query += ' ORDER BY created_at DESC';
    const res = await this.pool.query(query, params);
    return res.rows;
  }

  async deleteExpiredPermissionGrants() {
    const res = await this.pool.query(
      'DELETE FROM permission_grants WHERE expires_at < NOW() RETURNING *'
    );
    return res.rows;
  }

  // ── Workspace Usage (Story 3.12) ─────────────────────────────────────────

  async updateWorkspaceUsage(instanceId: string, usedGB: number) {
    await this.pool.query(
      'UPDATE agent_instances SET workspace_used_gb = $2, updated_at = NOW() WHERE id = $1',
      [instanceId, usedGB]
    );
  }

  // ── Cleanup Helpers ───────────────────────────────────────────────────────

  async deleteTemplatesExcept(validNames: string[]) {
    const all = await this.listTemplates();
    const toRemove = all.filter((t) => !validNames.includes(t.name) && !t.builtin);
    const removed: string[] = [];
    const errors: string[] = [];

    for (const t of toRemove) {
      try {
        await this.deleteTemplate(t.name);
        removed.push(t.name);
      } catch (err: unknown) {
        errors.push(`${t.name}: ${(err as Error).message}`);
      }
    }
    return { removed, errors };
  }

  async deleteNamedListsExcept(validNames: string[]) {
    const res = await this.pool.query("SELECT name FROM named_lists WHERE source = 'file'");
    const toRemove = res.rows.filter((r) => !validNames.includes(r.name)).map((r) => r.name);
    if (toRemove.length === 0) return { removed: [], errors: [] };

    try {
      const delRes = await this.pool.query(
        "DELETE FROM named_lists WHERE source = 'file' AND name = ANY($1) RETURNING name",
        [toRemove]
      );
      return { removed: delRes.rows.map((r) => r.name), errors: [] };
    } catch (err: unknown) {
      logger.error('Failed to batch delete named lists:', err);
      return { removed: [], errors: [(err as Error).message] };
    }
  }

  async deleteCapabilityPoliciesExcept(validNames: string[]) {
    const res = await this.pool.query("SELECT name FROM capability_policies WHERE source = 'file'");
    const toRemove = res.rows.filter((r) => !validNames.includes(r.name)).map((r) => r.name);
    if (toRemove.length === 0) return { removed: [], errors: [] };

    try {
      const delRes = await this.pool.query(
        "DELETE FROM capability_policies WHERE source = 'file' AND name = ANY($1) RETURNING name",
        [toRemove]
      );
      return { removed: delRes.rows.map((r) => r.name), errors: [] };
    } catch (err: unknown) {
      logger.error('Failed to batch delete capability policies:', err);
      return { removed: [], errors: [(err as Error).message] };
    }
  }

  async deleteSandboxBoundariesExcept(validNames: string[]) {
    const res = await this.pool.query("SELECT name FROM sandbox_boundaries WHERE source = 'file'");
    const toRemove = res.rows.filter((r) => !validNames.includes(r.name)).map((r) => r.name);
    if (toRemove.length === 0) return { removed: [], errors: [] };

    try {
      const delRes = await this.pool.query(
        "DELETE FROM sandbox_boundaries WHERE source = 'file' AND name = ANY($1) RETURNING name",
        [toRemove]
      );
      return { removed: delRes.rows.map((r) => r.name), errors: [] };
    } catch (err: unknown) {
      logger.error('Failed to batch delete sandbox boundaries:', err);
      return { removed: [], errors: [(err as Error).message] };
    }
  }

  // ── Template Diff & Update ────────────────────────────────────────────────

  async getTemplateDiff(instanceId: string): Promise<TemplateDiff> {
    const instanceRes = await this.pool.query(
      'SELECT id, name, template_ref, resolved_config, template_applied_at FROM agent_instances WHERE id = $1',
      [instanceId]
    );
    const instance = instanceRes.rows[0];
    if (!instance)
      throw Object.assign(new Error(`Instance ${instanceId} not found`), { status: 404 });

    const templateRef = instance.template_ref as string;
    const templateRes = await this.pool.query(
      'SELECT name, spec, updated_at FROM agent_templates WHERE name = $1',
      [templateRef]
    );
    const template = templateRes.rows[0];
    if (!template)
      throw Object.assign(new Error(`Template ${templateRef} not found`), { status: 404 });

    const templateUpdatedAt = (template.updated_at as Date).toISOString();
    const instanceAppliedAt = instance.template_applied_at
      ? (instance.template_applied_at as Date).toISOString()
      : null;

    const templateSpec = (template.spec ?? {}) as Record<string, unknown>;
    const appliedSpec = (instance.resolved_config ?? {}) as Record<string, unknown>;

    const changes: TemplateDiffChange[] = [];
    deepDiff(appliedSpec, templateSpec, '', changes);

    const hasChanges =
      instanceAppliedAt === null || new Date(templateUpdatedAt) > new Date(instanceAppliedAt);

    return {
      hasChanges,
      instanceId,
      templateName: templateRef,
      templateUpdatedAt,
      instanceAppliedAt,
      changes,
    };
  }

  async applyTemplateUpdate(instanceId: string, paths?: string[]): Promise<void> {
    const diff = await this.getTemplateDiff(instanceId);

    if (!diff.hasChanges) return;

    const templateRes = await this.pool.query('SELECT spec FROM agent_templates WHERE name = $1', [
      diff.templateName,
    ]);
    const template = templateRes.rows[0];
    if (!template)
      throw Object.assign(new Error(`Template ${diff.templateName} not found`), { status: 404 });

    const templateSpec = (template.spec ?? {}) as Record<string, unknown>;
    let nextConfig: Record<string, unknown>;

    if (paths && paths.length > 0) {
      // Partial apply: fetch current instance config and overlay only the requested paths
      const instanceRes = await this.pool.query(
        'SELECT resolved_config FROM agent_instances WHERE id = $1',
        [instanceId]
      );
      const inst = instanceRes.rows[0];
      const current = (inst?.resolved_config ?? {}) as Record<string, unknown>;
      nextConfig = { ...current };

      for (const path of paths) {
        const keys = path.split('.');
        let src: any = templateSpec;
        let dst: any = nextConfig;

        for (let i = 0; i < keys.length - 1; i++) {
          const key = keys[i]!;
          src = (src as Record<string, unknown>)[key];
          if (!(key in dst) || typeof dst[key] !== 'object' || dst[key] === null) {
            dst[key] = {};
          }
          dst = dst[key] as Record<string, unknown>;
        }

        const lastKey = keys[keys.length - 1]!;
        dst[lastKey] = (src as Record<string, unknown>)[lastKey];
      }
    } else {
      nextConfig = templateSpec;
    }

    await this.pool.query(
      'UPDATE agent_instances SET resolved_config = $2, template_applied_at = NOW(), updated_at = NOW() WHERE id = $1',
      [instanceId, nextConfig]
    );

    // Re-sync schedules and budgets from the newly merged config
    await this.syncManifestSchedules(instanceId, nextConfig, diff.templateName);
    await this.syncManifestBudget(instanceId, nextConfig);
  }

  async skipTemplateUpdate(instanceId: string): Promise<void> {
    const instanceRes = await this.pool.query(
      'SELECT template_ref FROM agent_instances WHERE id = $1',
      [instanceId]
    );
    const instance = instanceRes.rows[0];
    if (!instance)
      throw Object.assign(new Error(`Instance ${instanceId} not found`), { status: 404 });

    const templateRes = await this.pool.query(
      'SELECT updated_at FROM agent_templates WHERE name = $1',
      [instance.template_ref]
    );
    const template = templateRes.rows[0];
    if (!template)
      throw Object.assign(new Error(`Template ${instance.template_ref} not found`), {
        status: 404,
      });

    await this.pool.query(
      'UPDATE agent_instances SET template_applied_at = $2, updated_at = NOW() WHERE id = $1',
      [instanceId, template.updated_at]
    );
  }

  async getInstancesWithPendingUpdates(): Promise<PendingUpdateEntry[]> {
    const res = await this.pool.query(`
      SELECT
        ai.id AS "instanceId",
        ai.name AS "instanceName",
        at.name AS "templateName",
        at.updated_at AS "templateUpdatedAt"
      FROM agent_instances ai
      JOIN agent_templates at ON at.name = ai.template_ref
      WHERE ai.template_applied_at IS NULL
         OR at.updated_at > ai.template_applied_at
      ORDER BY at.updated_at DESC
    `);
    return (
      res.rows as Array<{
        instanceId: string;
        instanceName: string;
        templateName: string;
        templateUpdatedAt: Date;
      }>
    ).map((r) => ({
      instanceId: r.instanceId,
      instanceName: r.instanceName,
      templateName: r.templateName,
      templateUpdatedAt: r.templateUpdatedAt.toISOString(),
    }));
  }
}
