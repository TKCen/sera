/**
 * Migration: Epic 04 — LLM Proxy & Governance
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */

exports.up = (pgm) => {
  // Extend usage_events with metering columns
  pgm.sql(`
    ALTER TABLE usage_events
      ADD COLUMN IF NOT EXISTS cost_usd   numeric(10,6),
      ADD COLUMN IF NOT EXISTS latency_ms int,
      ADD COLUMN IF NOT EXISTS status     text NOT NULL DEFAULT 'success';
    CREATE INDEX IF NOT EXISTS usage_events_agent_time_idx ON usage_events (agent_id, created_at DESC);
  `);
};

exports.down = (pgm) => {
  pgm.dropIndex('usage_events', [], { name: 'usage_events_agent_time_idx', ifExists: true });
  pgm.dropColumns('usage_events', ['cost_usd', 'latency_ms', 'status']);
};
