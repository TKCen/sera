/**
 * Migration: Epic 05 — Agent Runtime
 *
 * Story 5.8: task_queue table for persistent agent work queues.
 * Story 5.9: task result storage columns (thought_stream, result_truncated).
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
    task: {
      type: 'text',
      notNull: true,
    },
    context: {
      type: 'jsonb',
      notNull: false,
    },
    status: {
      type: 'text',
      notNull: true,
      default: "'queued'",
      check: "status IN ('queued', 'running', 'completed', 'failed')",
    },
    priority: {
      type: 'int',
      notNull: true,
      default: 100,
    },
    retry_count: {
      type: 'int',
      notNull: true,
      default: 0,
    },
    max_retries: {
      type: 'int',
      notNull: true,
      default: 3,
    },
    created_at: {
      type: 'timestamptz',
      notNull: true,
      default: pgm.func('now()'),
    },
    started_at: {
      type: 'timestamptz',
      notNull: false,
    },
    completed_at: {
      type: 'timestamptz',
      notNull: false,
    },
    // Full agent output (Story 5.9)
    result: {
      type: 'jsonb',
      notNull: false,
    },
    // Error message on failure
    error: {
      type: 'text',
      notNull: false,
    },
    // Token usage summary (Story 5.9)
    usage: {
      type: 'jsonb',
      notNull: false,
    },
    // Ordered thought events from the reasoning loop (Story 5.9)
    thought_stream: {
      type: 'jsonb',
      notNull: false,
    },
    // True when result was truncated due to size limit (Story 5.9)
    result_truncated: {
      type: 'boolean',
      notNull: true,
      default: false,
    },
    // Reason the task exited (Story 5.9)
    exit_reason: {
      type: 'text',
      notNull: false,
    },
  });

  // Primary query pattern: agent tasks by status, priority, time
  pgm.createIndex('task_queue', ['agent_instance_id', 'status', 'priority', 'created_at'], {
    name: 'task_queue_agent_status_priority_idx',
  });

  // Dead-letter / retry query pattern
  pgm.createIndex('task_queue', ['status', 'retry_count'], {
    name: 'task_queue_retry_idx',
  });
};

exports.down = (pgm) => {
  pgm.dropTable('task_queue');
};
