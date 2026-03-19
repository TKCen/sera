/**
 * Migration: Epic 09 — Real-Time Messaging
 * Story 9.7: thought_events table.
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */
exports.up = (pgm) => {
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
};

exports.down = (pgm) => {
  pgm.dropTable('thought_events');
};
