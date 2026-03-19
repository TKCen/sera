/**
 * Migration: Epic 10 - Circles & Coordination
 */
exports.up = (pgm) => {
  // ── Circles Table ───────────────────────────────────────────────────────
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

  // ── Agent Instances Update ──────────────────────────────────────────────
  // Add circle_id to track current membership, especially for ephemeral agents.
  pgm.addColumns('agent_instances', {
    circle_id: { type: 'uuid', references: 'circles', onDelete: 'SET NULL' },
  }, { ifNotExists: true });

  // ── Coordination/Pipeline State (Story 10.3) ──────────────────────────
  pgm.createTable('pipelines', {
    id: { type: 'uuid', primaryKey: true },
    type: { type: 'text', notNull: true }, // sequential, parallel, hierarchical
    status: { type: 'text', notNull: true, default: 'pending' },
    steps: { type: 'jsonb', notNull: true, default: '[]' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    completed_at: { type: 'timestamptz' },
  }, { ifNotExists: true });

  // ── Party Sessions (Story 10.6) ────────────────────────────────────────────
  pgm.createTable('party_sessions', {
    id: { type: 'uuid', primaryKey: true },
    circle_id: { type: 'uuid', notNull: true, references: 'circles', onDelete: 'CASCADE' },
    prompt: { type: 'text', notNull: true },
    rounds: { type: 'jsonb', notNull: true, default: "'[]'" },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    completed_at: { type: 'timestamptz' },
  }, { ifNotExists: true });

  pgm.sql('CREATE INDEX IF NOT EXISTS party_sessions_circle_idx ON party_sessions (circle_id)');
};

exports.down = (pgm) => {
  pgm.dropTable('party_sessions');
  pgm.dropTable('pipelines');
  pgm.dropColumns('agent_instances', ['circle_id']);
  pgm.dropTable('circles');
};
