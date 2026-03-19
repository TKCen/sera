/**
 * Migration: Epic 02 — Agent Manifest & Registry
 *
 * ALL createTable calls use { ifNotExists: true }.
 * addColumns uses IF NOT EXISTS via raw SQL to be safe on re-runs.
 * See docs/MIGRATIONS.md for the project-wide idempotency policy.
 */
exports.up = (pgm) => {
  // ── Agent Templates ──────────────────────────────────────────────────────
  pgm.createTable('agent_templates', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    display_name: { type: 'text' },
    builtin: { type: 'boolean', notNull: true, default: false },
    category: { type: 'text' },
    spec: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  // ── Named Lists ──────────────────────────────────────────────────────────
  pgm.createTable('named_lists', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    type: { type: 'text', notNull: true },
    source: { type: 'text', notNull: true, default: 'file' },
    entries: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  // ── Capability Policies ───────────────────────────────────────────────────
  pgm.createTable('capability_policies', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    source: { type: 'text', notNull: true, default: 'file' },
    capabilities: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  // ── Sandbox Boundaries ───────────────────────────────────────────────────
  pgm.createTable('sandbox_boundaries', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    source: { type: 'text', notNull: true, default: 'file' },
    linux: { type: 'jsonb', notNull: true },
    capabilities: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  // ── Update Agent Instances — add columns only if they do not exist ──────
  // Using raw SQL with IF NOT EXISTS is the safest approach for addColumns.
  pgm.sql(`
    ALTER TABLE agent_instances
      ADD COLUMN IF NOT EXISTS display_name text,
      ADD COLUMN IF NOT EXISTS template_ref text,
      ADD COLUMN IF NOT EXISTS circle text,
      ADD COLUMN IF NOT EXISTS sandbox_boundary text,
      ADD COLUMN IF NOT EXISTS lifecycle_mode text DEFAULT 'persistent',
      ADD COLUMN IF NOT EXISTS parent_instance_id uuid REFERENCES agent_instances ON DELETE CASCADE,
      ADD COLUMN IF NOT EXISTS overrides jsonb DEFAULT '{}',
      ADD COLUMN IF NOT EXISTS resolved_config jsonb,
      ADD COLUMN IF NOT EXISTS resolved_capabilities jsonb,
      ADD COLUMN IF NOT EXISTS owner_sub text;
  `);
};

exports.down = (pgm) => {
  pgm.dropColumns('agent_instances', [
    'display_name', 'template_ref', 'circle', 'sandbox_boundary',
    'lifecycle_mode', 'parent_instance_id', 'overrides',
    'resolved_config', 'resolved_capabilities', 'owner_sub',
  ]);
  pgm.dropTable('sandbox_boundaries');
  pgm.dropTable('capability_policies');
  pgm.dropTable('named_lists');
  pgm.dropTable('agent_templates');
};
