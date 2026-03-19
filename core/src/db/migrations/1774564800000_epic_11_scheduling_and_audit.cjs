/**
 * Migration: Epic 11 — Scheduling & Audit
 *
 * Story 11.1: schedules table for agent tasks.
 * Story 11.4: audit_trail table with Merkle hash-chain.
 */

exports.up = (pgm) => {
  // ── Schedules ───────────────────────────────────────────────────────────
  pgm.createTable('schedules', {
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
    agent_name: {
      type: 'text',
      notNull: true,
    },
    name: {
      type: 'text',
      notNull: true,
    },
    description: {
      type: 'text',
    },
    type: {
      type: 'text',
      notNull: true,
      check: "type IN ('cron', 'once')",
    },
    expression: {
      type: 'text',
      notNull: true,
    },
    task: {
      type: 'text',
      notNull: true,
    },
    status: {
      type: 'text',
      notNull: true,
      default: "'active'",
      check: "status IN ('active', 'paused', 'completed', 'error')",
    },
    source: {
      type: 'text',
      notNull: true,
      default: "'api'",
      check: "source IN ('manifest', 'api')",
    },
    last_run_at: {
      type: 'timestamptz',
    },
    next_run_at: {
      type: 'timestamptz',
    },
    last_run_status: {
      type: 'text',
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

  // Unique name per agent
  pgm.createIndex('schedules', ['agent_instance_id', 'name'], { unique: true });
  pgm.createIndex('schedules', ['next_run_at', 'status']);

  // ── Audit Trail ────────────────────────────────────────────────────────
  pgm.createTable('audit_trail', {
    id: {
      type: 'uuid',
      primaryKey: true,
      default: pgm.func('gen_random_uuid()'),
    },
    sequence: {
      type: 'bigserial',
      notNull: true,
      unique: true,
    },
    timestamp: {
      type: 'timestamptz',
      notNull: true,
      default: pgm.func('now()'),
    },
    actor_type: {
      type: 'text',
      notNull: true,
      check: "actor_type IN ('operator', 'agent', 'system')",
    },
    actor_id: {
      type: 'text',
      notNull: true,
    },
    acting_context: {
      type: 'jsonb',
      notNull: false,
    },
    event_type: {
      type: 'text',
      notNull: true,
    },
    payload: {
      type: 'jsonb',
      notNull: true,
    },
    prev_hash: {
      type: 'text',
    },
    hash: {
      type: 'text',
      notNull: true,
    },
  });

  pgm.createIndex('audit_trail', ['sequence']);
  pgm.createIndex('audit_trail', ['actor_id', 'event_type', 'timestamp']);
};

exports.down = (pgm) => {
  pgm.dropTable('audit_trail');
  pgm.dropTable('schedules');
};
