/**
 * Migration: Initial Schema
 *
 * ALL createTable calls use { ifNotExists: true }.
 * ALL createIndex calls use CREATE INDEX IF NOT EXISTS via raw SQL.
 * This is the project-wide pattern — see docs/MIGRATIONS.md.
 */
exports.up = (pgm) => {
  // Enable pgvector extension
  pgm.sql('CREATE EXTENSION IF NOT EXISTS vector');

  // ── Embeddings ──────────────────────────────────────────────────────────
  pgm.createTable('embeddings', {
    id: 'id',
    content: { type: 'text', notNull: true },
    metadata: { type: 'jsonb' },
    embedding: { type: 'vector(1536)' },
    created_at: {
      type: 'timestamp with time zone',
      notNull: true,
      default: pgm.func('current_timestamp'),
    },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS embeddings_vector_idx ON embeddings USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100)');

  // ── Agent Instances ─────────────────────────────────────────────────────
  pgm.createTable('agent_instances', {
    id: { type: 'uuid', primaryKey: true },
    template_name: { type: 'text', notNull: true },
    name: { type: 'text', notNull: true },
    workspace_path: { type: 'text', notNull: true },
    container_id: { type: 'text' },
    status: { type: 'text', default: 'active' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  // ── Chat Sessions ───────────────────────────────────────────────────────
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

  // ── Chat Messages ───────────────────────────────────────────────────────
  pgm.createTable('chat_messages', {
    id: { type: 'uuid', primaryKey: true },
    session_id: { type: 'uuid', notNull: true, references: 'chat_sessions', onDelete: 'CASCADE' },
    role: { type: 'text', notNull: true },
    content: { type: 'text', notNull: true },
    metadata: { type: 'jsonb' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS chat_messages_session_created_idx ON chat_messages (session_id, created_at)');

  // ── Token Usage & Quotas ──────────────────────────────────────────────
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

  // ── Usage Events ────────────────────────────────────────────────────────
  pgm.createTable('usage_events', {
    id: 'id',
    agent_id: { type: 'text', notNull: true },
    model: { type: 'text', notNull: true },
    prompt_tokens: { type: 'int', notNull: true, default: 0 },
    completion_tokens: { type: 'int', notNull: true, default: 0 },
    total_tokens: { type: 'int', notNull: true, default: 0 },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS usage_events_agent_created_idx ON usage_events (agent_id, created_at DESC)');

  // ── Audit Trail ─────────────────────────────────────────────────────────
  pgm.createTable('audit_trail', {
    id: 'id',
    agent_id: { type: 'text', notNull: true },
    action: { type: 'text', notNull: true },
    details: { type: 'jsonb' },
    timestamp: { type: 'timestamptz', default: pgm.func('now()') },
    previous_hash: { type: 'text' },
    hash: { type: 'text', notNull: true },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS audit_trail_agent_time_idx ON audit_trail (agent_id, timestamp)');

  // ── Schedules ───────────────────────────────────────────────────────────
  pgm.createTable('schedules', {
    id: { type: 'uuid', primaryKey: true },
    agent_id: { type: 'uuid', references: 'agent_instances', onDelete: 'CASCADE' },
    name: { type: 'text', notNull: true },
    cron: { type: 'text', notNull: true },
    task: { type: 'jsonb', notNull: true },
    status: { type: 'text', default: 'active' },
    last_run: { type: 'timestamptz' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS schedules_agent_status_idx ON schedules (agent_id, status)');
};

exports.down = (pgm) => {
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
