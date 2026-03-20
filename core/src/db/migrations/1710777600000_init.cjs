/**
 * Migration: Consolidated Init
 *
 * Single migration combining all epics into one clean fresh-start schema.
 * Replaces 15 individual migration files (epics 01–16).
 * All DDL uses IF NOT EXISTS for idempotency.
 */
exports.up = (pgm) => {
  // ── Extensions ──────────────────────────────────────────────────────────
  pgm.sql('CREATE EXTENSION IF NOT EXISTS vector');

  // ── Embeddings ──────────────────────────────────────────────────────────
  pgm.createTable('embeddings', {
    id: 'id',
    content: { type: 'text', notNull: true },
    metadata: { type: 'jsonb' },
    embedding: { type: 'vector(1536)' },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS embeddings_vector_idx ON embeddings USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100)');

  // ── Agent Instances ──────────────────────────────────────────────────────
  // NOTE: circle_id FK added after circles table is created (below).
  pgm.createTable('agent_instances', {
    id: { type: 'uuid', primaryKey: true },
    template_name: { type: 'text', notNull: true },
    name: { type: 'text', notNull: true },
    workspace_path: { type: 'text', notNull: true },
    container_id: { type: 'text' },
    status: { type: 'text', default: 'active' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
    // Epic 02: manifest & registry columns
    display_name: { type: 'text' },
    template_ref: { type: 'text' },
    circle: { type: 'text' },
    sandbox_boundary: { type: 'text' },
    lifecycle_mode: { type: 'text', default: 'persistent' },
    parent_instance_id: { type: 'uuid', references: 'agent_instances', onDelete: 'CASCADE' },
    overrides: { type: 'jsonb', default: '{}' },
    resolved_config: { type: 'jsonb' },
    resolved_capabilities: { type: 'jsonb' },
    owner_sub: { type: 'text' },
    // Epic 03: lifecycle & workspace columns
    last_heartbeat_at: { type: 'timestamptz' },
    workspace_used_gb: { type: 'numeric(10,3)' },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS agent_instances_last_heartbeat_idx ON agent_instances (last_heartbeat_at)');

  // ── Chat Sessions ────────────────────────────────────────────────────────
  pgm.createTable('chat_sessions', {
    id: { type: 'uuid', primaryKey: true },
    agent_name: { type: 'text', notNull: true },
    agent_instance_id: { type: 'uuid', references: 'agent_instances', onDelete: 'SET NULL' },
    title: { type: 'text', notNull: true, default: 'New Chat' },
    message_count: { type: 'int', default: 0 },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS chat_sessions_agent_updated_idx ON chat_sessions (agent_name, updated_at DESC)');

  // ── Chat Messages ────────────────────────────────────────────────────────
  pgm.createTable('chat_messages', {
    id: { type: 'uuid', primaryKey: true },
    session_id: { type: 'uuid', notNull: true, references: 'chat_sessions', onDelete: 'CASCADE' },
    role: { type: 'text', notNull: true },
    content: { type: 'text', notNull: true },
    metadata: { type: 'jsonb' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS chat_messages_session_created_idx ON chat_messages (session_id, created_at)');

  // ── Token Usage & Quotas ─────────────────────────────────────────────────
  pgm.createTable('token_usage', {
    id: 'id',
    agent_id: { type: 'text', notNull: true },
    circle_id: { type: 'text' },
    model: { type: 'text', notNull: true },
    prompt_tokens: { type: 'int', notNull: true, default: 0 },
    completion_tokens: { type: 'int', notNull: true, default: 0 },
    total_tokens: { type: 'int', notNull: true, default: 0 },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS token_usage_agent_created_idx ON token_usage (agent_id, created_at DESC)');

  pgm.createTable('token_quotas', {
    agent_id: { type: 'text', primaryKey: true },
    max_tokens_per_hour: { type: 'int', notNull: true, default: 100000 },
    max_tokens_per_day: { type: 'int', notNull: true, default: 1000000 },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  // ── Usage Events (Epic 04: metering columns included) ───────────────────
  pgm.createTable('usage_events', {
    id: 'id',
    agent_id: { type: 'text', notNull: true },
    model: { type: 'text', notNull: true },
    prompt_tokens: { type: 'int', notNull: true, default: 0 },
    completion_tokens: { type: 'int', notNull: true, default: 0 },
    total_tokens: { type: 'int', notNull: true, default: 0 },
    cost_usd: { type: 'numeric(10,6)' },
    latency_ms: { type: 'int' },
    status: { type: 'text', notNull: true, default: 'success' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS usage_events_agent_time_idx ON usage_events (agent_id, created_at DESC)');

  // ── Audit Trail (Epic 11 Merkle hash-chain schema) ───────────────────────
  pgm.sql(`
    CREATE TABLE IF NOT EXISTS audit_trail (
      id             uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
      sequence       bigserial   NOT NULL UNIQUE,
      timestamp      timestamptz NOT NULL DEFAULT now(),
      actor_type     text        NOT NULL CHECK (actor_type IN ('operator', 'agent', 'system')),
      actor_id       text        NOT NULL,
      acting_context jsonb,
      event_type     text        NOT NULL,
      payload        jsonb       NOT NULL,
      prev_hash      text,
      hash           text        NOT NULL
    );
    CREATE INDEX IF NOT EXISTS audit_trail_sequence_idx         ON audit_trail (sequence);
    CREATE INDEX IF NOT EXISTS audit_trail_actor_event_time_idx ON audit_trail (actor_id, event_type, timestamp);
  `);

  // ── Schedules (Epic 11 full schema) ─────────────────────────────────────
  pgm.createTable('schedules', {
    id: { type: 'uuid', primaryKey: true },
    agent_id: { type: 'uuid', references: 'agent_instances', onDelete: 'CASCADE' },
    agent_instance_id: { type: 'uuid', references: 'agent_instances', onDelete: 'CASCADE' },
    agent_name: { type: 'text' },
    name: { type: 'text', notNull: true },
    cron: { type: 'text' },
    expression: { type: 'text' },
    type: { type: 'text', default: 'cron' },
    task: { type: 'jsonb', notNull: true },
    source: { type: 'text', notNull: true, default: 'api' },
    status: { type: 'text', default: 'active' },
    last_run: { type: 'timestamptz' },
    last_run_at: { type: 'timestamptz' },
    last_run_status: { type: 'text' },
    next_run_at: { type: 'timestamptz' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE INDEX IF NOT EXISTS schedules_agent_status_idx ON schedules (agent_id, status);
    CREATE UNIQUE INDEX IF NOT EXISTS schedules_agent_instance_name_key
      ON schedules (agent_instance_id, name) WHERE agent_instance_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS schedules_next_run_at_status_idx ON schedules (next_run_at, status);
  `);

  // ── Agent Templates ───────────────────────────────────────────────────────
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

  // ── Named Lists ───────────────────────────────────────────────────────────
  pgm.createTable('named_lists', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    type: { type: 'text', notNull: true },
    source: { type: 'text', notNull: true, default: 'file' },
    entries: { type: 'jsonb', notNull: true },
    always_enforced: { type: 'boolean', notNull: true, default: false },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS named_lists_always_enforced_idx ON named_lists (always_enforced)');

  // ── Capability Policies ───────────────────────────────────────────────────
  pgm.createTable('capability_policies', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    source: { type: 'text', notNull: true, default: 'file' },
    capabilities: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  // ── Sandbox Boundaries ────────────────────────────────────────────────────
  pgm.createTable('sandbox_boundaries', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    source: { type: 'text', notNull: true, default: 'file' },
    linux: { type: 'jsonb', notNull: true },
    capabilities: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  // ── API Keys ──────────────────────────────────────────────────────────────
  pgm.createTable('api_keys', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true },
    key_hash: { type: 'text', notNull: true },
    owner_sub: { type: 'text', notNull: true },
    roles: { type: 'text[]', notNull: true, default: '{}' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    expires_at: { type: 'timestamptz' },
    last_used_at: { type: 'timestamptz' },
    revoked_at: { type: 'timestamptz' },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE INDEX IF NOT EXISTS api_keys_owner_idx ON api_keys (owner_sub);
    CREATE INDEX IF NOT EXISTS api_keys_hash_idx  ON api_keys (key_hash);
  `);

  // ── Secrets ───────────────────────────────────────────────────────────────
  pgm.createTable('secrets', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    encrypted_value: { type: 'bytea', notNull: true },
    iv: { type: 'bytea', notNull: true },
    description: { type: 'text' },
    allowed_agents: { type: 'text[]', notNull: true, default: '{}' },
    tags: { type: 'text[]', notNull: true, default: '{}' },
    exposure: { type: 'text', notNull: true, default: 'per-call' },
    created_by: { type: 'text' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
    rotated_at: { type: 'timestamptz' },
    expires_at: { type: 'timestamptz' },
    deleted_at: { type: 'timestamptz' },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS secrets_name_idx ON secrets (name)');

  // ── Capability Grants (Epic 03 + Epic 16 identity columns) ───────────────
  pgm.createTable('capability_grants', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    agent_instance_id: { type: 'uuid', notNull: true },
    dimension: { type: 'varchar(64)', notNull: true },
    value: { type: 'text', notNull: true },
    grant_type: { type: 'varchar(16)', notNull: true, check: "grant_type IN ('one-time', 'session', 'persistent')" },
    granted_by: { type: 'text' },
    granted_by_email: { type: 'text' },
    granted_by_name: { type: 'text' },
    expires_at: { type: 'timestamptz' },
    revoked_at: { type: 'timestamptz' },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE INDEX IF NOT EXISTS capability_grants_agent_idx         ON capability_grants (agent_instance_id);
    CREATE INDEX IF NOT EXISTS capability_grants_agent_revoked_idx ON capability_grants (agent_instance_id, revoked_at);
  `);

  // ── Task Queue (Epic 05) ──────────────────────────────────────────────────
  pgm.createTable('task_queue', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    agent_instance_id: { type: 'uuid', notNull: true, references: '"agent_instances"', onDelete: 'CASCADE' },
    task: { type: 'text', notNull: true },
    context: { type: 'jsonb' },
    status: { type: 'text', notNull: true, default: "'queued'", check: "status IN ('queued', 'running', 'completed', 'failed')" },
    priority: { type: 'int', notNull: true, default: 100 },
    retry_count: { type: 'int', notNull: true, default: 0 },
    max_retries: { type: 'int', notNull: true, default: 3 },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    started_at: { type: 'timestamptz' },
    completed_at: { type: 'timestamptz' },
    result: { type: 'jsonb' },
    error: { type: 'text' },
    usage: { type: 'jsonb' },
    thought_stream: { type: 'jsonb' },
    result_truncated: { type: 'boolean', notNull: true, default: false },
    exit_reason: { type: 'text' },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE INDEX IF NOT EXISTS task_queue_agent_status_priority_idx
      ON task_queue (agent_instance_id, status, priority, created_at);
    CREATE INDEX IF NOT EXISTS task_queue_retry_idx
      ON task_queue (status, retry_count);
  `);

  // ── Thought Events (Epic 09) ──────────────────────────────────────────────
  pgm.createTable('thought_events', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    agent_instance_id: { type: 'uuid', notNull: true, references: 'agent_instances', onDelete: 'CASCADE' },
    task_id: { type: 'text' },
    step: { type: 'text', notNull: true },
    content: { type: 'text', notNull: true },
    iteration: { type: 'int', notNull: true, default: 0 },
    published_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE INDEX IF NOT EXISTS thought_events_agent_published_idx ON thought_events (agent_instance_id, published_at);
    CREATE INDEX IF NOT EXISTS thought_events_task_idx            ON thought_events (task_id);
  `);

  // ── Webhooks & Deliveries (Epic 09) ───────────────────────────────────────
  pgm.createTable('webhooks', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true },
    url_path: { type: 'text', notNull: true, unique: true },
    secret: { type: 'text', notNull: true },
    event_type: { type: 'text', notNull: true },
    enabled: { type: 'boolean', notNull: true, default: true },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  }, { ifNotExists: true });

  pgm.createTable('webhook_deliveries', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    webhook_id: { type: 'uuid', notNull: true, references: 'webhooks', onDelete: 'CASCADE' },
    payload: { type: 'jsonb', notNull: true },
    status: { type: 'text', notNull: true },
    error_message: { type: 'text' },
    processed_at: { type: 'timestamptz' },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE INDEX IF NOT EXISTS webhooks_url_path_idx             ON webhooks (url_path);
    CREATE INDEX IF NOT EXISTS webhook_deliveries_wh_created_idx ON webhook_deliveries (webhook_id, created_at);
  `);

  // ── Skills (Epic 06) ──────────────────────────────────────────────────────
  pgm.createTable('skills', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    skill_id: { type: 'text' },
    name: { type: 'text', notNull: true },
    version: { type: 'text', notNull: true },
    description: { type: 'text', notNull: true },
    triggers: { type: 'jsonb', notNull: true, default: '[]' },
    requires: { type: 'jsonb', default: '[]' },
    conflicts: { type: 'jsonb', default: '[]' },
    max_tokens: { type: 'integer' },
    content: { type: 'text', notNull: true },
    source: { type: 'text', notNull: true, default: 'external' },
    category: { type: 'text' },
    tags: { type: 'jsonb', default: '[]' },
    applies_to: { type: 'jsonb', default: '[]' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE UNIQUE INDEX IF NOT EXISTS skills_name_version_idx ON skills (name, version);
    CREATE INDEX        IF NOT EXISTS skills_skill_id_idx     ON skills (skill_id);
  `);

  // ── Skill Packages (Epic 06) ──────────────────────────────────────────────
  pgm.createTable('skill_packages', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true },
    version: { type: 'text', notNull: true },
    description: { type: 'text' },
    skills: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE UNIQUE INDEX IF NOT EXISTS skill_packages_name_version_idx ON skill_packages (name, version)');

  // ── Knowledge Merge Requests (Epic 08) ────────────────────────────────────
  pgm.createTable('knowledge_merge_requests', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    circle_id: { type: 'text', notNull: true },
    agent_instance_id: { type: 'text', notNull: true },
    agent_name: { type: 'text', notNull: true },
    branch: { type: 'text', notNull: true },
    status: { type: 'text', notNull: true, default: 'pending' },
    approved_by: { type: 'text' },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    diff_summary: { type: 'text' },
    conflict_details: { type: 'jsonb' },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE INDEX IF NOT EXISTS kmr_circle_status_idx ON knowledge_merge_requests (circle_id, status);
    CREATE INDEX IF NOT EXISTS kmr_agent_idx         ON knowledge_merge_requests (agent_instance_id);
  `);

  // ── Circles (Epic 10) ─────────────────────────────────────────────────────
  pgm.createTable('circles', {
    id: { type: 'uuid', primaryKey: true },
    name: { type: 'text', notNull: true, unique: true },
    display_name: { type: 'text', notNull: true },
    description: { type: 'text' },
    constitution: { type: 'text' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS circles_name_idx ON circles (name)');

  // Add circle_id to agent_instances now that circles table exists
  pgm.sql('ALTER TABLE agent_instances ADD COLUMN IF NOT EXISTS circle_id uuid REFERENCES circles ON DELETE SET NULL');

  // ── Pipelines (Epic 10) ───────────────────────────────────────────────────
  pgm.createTable('pipelines', {
    id: { type: 'uuid', primaryKey: true },
    type: { type: 'text', notNull: true },
    status: { type: 'text', notNull: true, default: 'pending' },
    steps: { type: 'jsonb', notNull: true, default: '[]' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    completed_at: { type: 'timestamptz' },
  }, { ifNotExists: true });

  // ── Party Sessions (Epic 10) ──────────────────────────────────────────────
  pgm.createTable('party_sessions', {
    id: { type: 'uuid', primaryKey: true },
    circle_id: { type: 'uuid', notNull: true, references: 'circles', onDelete: 'CASCADE' },
    prompt: { type: 'text', notNull: true },
    rounds: { type: 'jsonb', notNull: true, default: '[]' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    completed_at: { type: 'timestamptz' },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS party_sessions_circle_idx ON party_sessions (circle_id)');
};

exports.down = (pgm) => {
  pgm.dropTable('party_sessions');
  pgm.dropTable('pipelines');
  pgm.sql('ALTER TABLE agent_instances DROP COLUMN IF EXISTS circle_id');
  pgm.dropTable('circles');
  pgm.dropTable('knowledge_merge_requests');
  pgm.dropTable('skill_packages');
  pgm.dropTable('skills');
  pgm.dropTable('webhook_deliveries');
  pgm.dropTable('webhooks');
  pgm.dropTable('thought_events');
  pgm.dropTable('task_queue');
  pgm.dropTable('capability_grants');
  pgm.dropTable('secrets');
  pgm.dropTable('api_keys');
  pgm.dropTable('sandbox_boundaries');
  pgm.dropTable('capability_policies');
  pgm.dropTable('named_lists');
  pgm.dropTable('agent_templates');
  pgm.dropTable('schedules');
  pgm.dropTable('audit_trail');
  pgm.dropTable('usage_events');
  pgm.dropTable('token_quotas');
  pgm.dropTable('token_usage');
  pgm.dropTable('chat_messages');
  pgm.dropTable('chat_sessions');
  pgm.dropTable('agent_instances');
  pgm.dropTable('embeddings');
  pgm.sql('DROP EXTENSION IF EXISTS vector');
};
