/**
 * Migration: Epic 03 — Lifecycle Updates
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */
exports.up = (pgm) => {
  pgm.sql(`
    ALTER TABLE named_lists     ADD COLUMN IF NOT EXISTS always_enforced boolean NOT NULL DEFAULT false;
    ALTER TABLE agent_instances ADD COLUMN IF NOT EXISTS last_heartbeat_at timestamptz;
    CREATE INDEX IF NOT EXISTS named_lists_always_enforced_idx     ON named_lists (always_enforced);
    CREATE INDEX IF NOT EXISTS agent_instances_last_heartbeat_idx  ON agent_instances (last_heartbeat_at);
  `);
};

exports.down = (pgm) => {
  pgm.dropColumn('agent_instances', 'last_heartbeat_at');
  pgm.dropColumn('named_lists', 'always_enforced');
};
