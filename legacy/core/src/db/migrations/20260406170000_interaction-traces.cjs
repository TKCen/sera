/**
 * Migration: Add interaction_traces table
 *
 * Stores full structured reasoning traces after each agent interaction.
 * Part of the closed-loop self-improvement system (Epic 30).
 */

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.up = (pgm) => {
  pgm.createTable('interaction_traces', {
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
    session_id: {
      type: 'text',
      notNull: true,
    },
    trace_data: {
      type: 'jsonb',
      notNull: true,
      default: '{}',
    },
    summary: {
      type: 'text',
      notNull: false,
    },
    token_count: {
      type: 'integer',
      notNull: true,
      default: 0,
    },
    created_at: {
      type: 'timestamptz',
      notNull: true,
      default: pgm.func('now()'),
    },
    updated_at: {
      type: 'timestamptz',
      notNull: true,
      default: pgm.func('now()'),
    },
  });

  pgm.createIndex('interaction_traces', 'agent_instance_id');
  pgm.createIndex('interaction_traces', 'session_id');
  pgm.createIndex('interaction_traces', ['agent_instance_id', 'session_id']);
  pgm.createIndex('interaction_traces', 'created_at');
};

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.down = (pgm) => {
  pgm.dropTable('interaction_traces');
};
