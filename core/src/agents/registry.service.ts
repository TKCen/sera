import type { Pool } from 'pg';
import type { AgentTemplate, AgentInstance, NamedList, CapabilityPolicy, SandboxBoundary } from './schemas.js';

export class AgentRegistry {
  constructor(private pool: Pool) {}

  // ── Agent Templates ──────────────────────────────────────────────────────

  async upsertTemplate(template: AgentTemplate) {
    const { name, displayName, builtin, category, description } = template.metadata;
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

    const { displayName, builtin, category, description } = template.metadata;
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

    // Check for active instances
    const instances = await this.listInstances();
    const referenced = instances.some(i => i.template_ref === name);
    if (referenced) {
      throw new Error(`Template ${name} is referenced by active instances`);
    }

    const res = await this.pool.query('DELETE FROM agent_templates WHERE name = $1 RETURNING *', [name]);
    return res.rows[0];
  }

  // ── Agent Instances ──────────────────────────────────────────────────────

  async createInstance(data: {
    name: string;
    displayName?: string;
    templateRef: string;
    circle?: string;
    overrides?: any;
    lifecycleMode?: 'persistent' | 'ephemeral';
    parentInstanceId?: string;
  }) {
    const id = crypto.randomUUID();
    const query = `
      INSERT INTO agent_instances (
        id, name, display_name, template_ref, circle, lifecycle_mode, parent_instance_id, overrides, status
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'created')
      RETURNING *;
    `;
    const res = await this.pool.query(query, [
      id,
      data.name,
      data.displayName,
      data.templateRef,
      data.circle,
      data.lifecycleMode || 'persistent',
      data.parentInstanceId,
      data.overrides || {},
    ]);
    return res.rows[0];
  }

  async getInstance(id: string) {
    const res = await this.pool.query('SELECT * FROM agent_instances WHERE id = $1', [id]);
    return res.rows[0];
  }

  async getInstanceByName(name: string) {
    const res = await this.pool.query('SELECT * FROM agent_instances WHERE name = $1', [name]);
    return res.rows[0];
  }

  async listInstances(filters: { circle?: string; status?: string } = {}) {
    let query = 'SELECT * FROM agent_instances';
    const params: any[] = [];
    const wheres: string[] = [];

    if (filters.circle) {
      params.push(filters.circle);
      wheres.push(`circle = $${params.length}`);
    }
    if (filters.status) {
      params.push(filters.status);
      wheres.push(`status = $${params.length}`);
    }

    if (wheres.length > 0) {
      query += ' WHERE ' + wheres.join(' AND ');
    }

    query += ' ORDER BY created_at DESC';
    const res = await this.pool.query(query, params);
    return res.rows;
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

  async updateInstanceConfig(id: string, overrides: any, resolvedConfig?: any, resolvedCapabilities?: any) {
    const query = `
      UPDATE agent_instances 
      SET overrides = $2, resolved_config = $3, resolved_capabilities = $4, updated_at = NOW()
      WHERE id = $1
      RETURNING *;
    `;
    const res = await this.pool.query(query, [id, overrides, resolvedConfig, resolvedCapabilities]);
    return res.rows[0];
  }

  async deleteInstance(id: string) {
    const res = await this.pool.query('DELETE FROM agent_instances WHERE id = $1 RETURNING *', [id]);
    return res.rows[0];
  }

  // ── Resources (NamedLists, Policies, Boundaries) ──────────────────────────

  async upsertNamedList(list: NamedList, source: string = 'file') {
    const { name, type } = list.metadata;
    const query = `
      INSERT INTO named_lists (name, type, source, entries, updated_at)
      VALUES ($1, $2, $3, $4, NOW())
      ON CONFLICT (name) DO UPDATE SET
        type = EXCLUDED.type,
        source = EXCLUDED.source,
        entries = EXCLUDED.entries,
        updated_at = NOW()
      RETURNING *;
    `;
    const res = await this.pool.query(query, [name, type, source, JSON.stringify(list.entries)]);
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

  async getNamedList(name: string) {
    const res = await this.pool.query('SELECT * FROM named_lists WHERE name = $1', [name]);
    return res.rows[0];
  }

  async getCapabilityPolicy(name: string) {
    const res = await this.pool.query('SELECT * FROM capability_policies WHERE name = $1', [name]);
    return res.rows[0];
  }

  async getSandboxBoundary(name: string) {
    const res = await this.pool.query('SELECT * FROM sandbox_boundaries WHERE name = $1', [name]);
    return res.rows[0];
  }
}
