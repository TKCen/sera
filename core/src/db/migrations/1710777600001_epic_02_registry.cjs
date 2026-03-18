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
  });

  // ── Named Lists ──────────────────────────────────────────────────────────
  pgm.createTable('named_lists', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    type: { type: 'text', notNull: true },
    source: { type: 'text', notNull: true, default: 'file' }, // 'file' | 'api' | 'builtin'
    entries: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  });

  // ── Capability Policies ───────────────────────────────────────────────────
  pgm.createTable('capability_policies', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    source: { type: 'text', notNull: true, default: 'file' },
    capabilities: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  });

  // ── Sandbox Boundaries ───────────────────────────────────────────────────
  pgm.createTable('sandbox_boundaries', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    source: { type: 'text', notNull: true, default: 'file' },
    linux: { type: 'jsonb', notNull: true },
    capabilities: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  });

  // ── Update Agent Instances ──────────────────────────────────────────────
  pgm.addColumns('agent_instances', {
    display_name: { type: 'text' },
    template_ref: { type: 'text' },
    circle: { type: 'text' },
    sandbox_boundary: { type: 'text' },
    lifecycle_mode: { type: 'text', default: 'persistent' },
    parent_instance_id: { type: 'uuid', references: 'agent_instances', onDelete: 'CASCADE' },
    overrides: { type: 'jsonb', default: '{}' },
    resolved_config: { type: 'jsonb' },
    resolved_capabilities: { type: 'jsonb' },
    owner_sub: { type: 'text' }, // Reserved for multi-user scoping
  });

  // Migrate template_name to template_ref if needed (optional for fresh env)
  // pgm.sql('UPDATE agent_instances SET template_ref = template_name');
};

exports.down = (pgm) => {
  pgm.dropColumns('agent_instances', [
    'display_name',
    'template_ref',
    'circle',
    'sandbox_boundary',
    'lifecycle_mode',
    'parent_instance_id',
    'overrides',
    'resolved_config',
    'resolved_capabilities',
    'owner_sub',
  ]);
  pgm.dropTable('sandbox_boundaries');
  pgm.dropTable('capability_policies');
  pgm.dropTable('named_lists');
  pgm.dropTable('agent_templates');
};
