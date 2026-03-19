/**
 * Migration: Epic 05 — Agent Runtime
 *
 * Story 5.8: task_queue table for persistent agent work queues.
 * Story 5.9: task result storage columns (thought_stream, result_truncated).
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */

exports.up = (pgm) => {
  pgm.createTable('task_queue', {
    id: {
      type: 'uuid',
      primaryKey: true,
      default: pgm.func('gen_random_uuid()'),
    },
    agent_instance_id: {
      type: 'uuid',
      notNull: true,
      references: '"agent_instances"',
      onDelete: 'CASCADE',
    },
    task: { type: 'text', notNull: true },
    context: { type: 'jsonb', notNull: false },
    status: {
      type: 'text',
      notNull: true,
      default: "'queued'",
      check: "status IN ('queued', 'running', 'completed', 'failed')",
    },
    priority: { type: 'int', notNull: true, default: 100 },
    retry_count: { type: 'int', notNull: true, default: 0 },
    max_retries: { type: 'int', notNull: true, default: 3 },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    started_at: { type: 'timestamptz', notNull: false },
    completed_at: { type: 'timestamptz', notNull: false },
    result: { type: 'jsonb', notNull: false },
    error: { type: 'text', notNull: false },
    usage: { type: 'jsonb', notNull: false },
    thought_stream: { type: 'jsonb', notNull: false },
    result_truncated: { type: 'boolean', notNull: true, default: false },
    exit_reason: { type: 'text', notNull: false },
  }, { ifNotExists: true });

  pgm.sql(`
    CREATE INDEX IF NOT EXISTS task_queue_agent_status_priority_idx
      ON task_queue (agent_instance_id, status, priority, created_at);
    CREATE INDEX IF NOT EXISTS task_queue_retry_idx
      ON task_queue (status, retry_count);
  `);
};

exports.down = (pgm) => {
  pgm.dropTable('task_queue');
};
