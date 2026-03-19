/**
 * Migration: Epic 11 — Scheduling & Audit
 *
 * Story 11.1: Evolves the schedules table (created in initial schema with
 *   basic columns) to the full Epic 11 schema, adding missing columns.
 * Story 11.4: Drops the minimal audit_trail from the initial schema and
 *   replaces it with the full Merkle hash-chain version.
 *
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 *
 * Background: The initial schema migration (1710777600000) created lightweight
 * versions of both schedules and audit_trail. This migration evolves them in-place
 * using ADD COLUMN IF NOT EXISTS so it is safe to run against both fresh and
 * pre-existing databases.
 */

exports.up = (pgm) => {
  // ── Schedules — evolve existing table ───────────────────────────────────
  // The initial schema created: id, agent_id, name, cron, task, status, last_run,
  // created_at, updated_at.
  // Epic 11 adds: agent_instance_id, agent_name, type, expression, source,
  // next_run_at, last_run_status, and replaces the simple 'cron' column with
  // an 'expression' column (keeping 'cron' for backward compat).
  pgm.sql(`
    ALTER TABLE schedules
      ADD COLUMN IF NOT EXISTS agent_instance_id uuid REFERENCES agent_instances ON DELETE CASCADE,
      ADD COLUMN IF NOT EXISTS agent_name        text,
      ADD COLUMN IF NOT EXISTS type              text DEFAULT 'cron',
      ADD COLUMN IF NOT EXISTS expression        text,
      ADD COLUMN IF NOT EXISTS source            text NOT NULL DEFAULT 'api',
      ADD COLUMN IF NOT EXISTS next_run_at       timestamptz,
      ADD COLUMN IF NOT EXISTS last_run_at       timestamptz,
      ADD COLUMN IF NOT EXISTS last_run_status   text;

    -- Back-fill expression from cron for pre-existing rows
    UPDATE schedules SET expression = cron WHERE expression IS NULL AND cron IS NOT NULL;

    -- Back-fill agent_instance_id from agent_id for pre-existing rows
    UPDATE schedules SET agent_instance_id = agent_id WHERE agent_instance_id IS NULL AND agent_id IS NOT NULL;

    -- Back-fill type
    UPDATE schedules SET type = 'cron' WHERE type IS NULL;

    -- Ensure status check constraint covers Epic 11 values (best-effort; skip if it exists)
    CREATE UNIQUE INDEX IF NOT EXISTS schedules_agent_instance_name_key
      ON schedules (agent_instance_id, name) WHERE agent_instance_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS schedules_next_run_at_status_idx
      ON schedules (next_run_at, status);
  `);

  // ── Audit Trail — replace minimal schema with full Merkle version ────────
  // The initial schema had: id (serial), agent_id, action, details, timestamp,
  // previous_hash, hash. Epic 11 needs a richer schema. We use CREATE TABLE IF NOT
  // EXISTS and rename the old table out of the way if it has the old shape.
  pgm.sql(`
    DO $$
    BEGIN
      -- If audit_trail still has the old 'agent_id' column it's the initial version.
      -- Rename it to audit_trail_v1 to preserve data, then create the new table.
      IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'audit_trail' AND column_name = 'agent_id'
      ) THEN
        ALTER TABLE audit_trail RENAME TO audit_trail_v1;
      END IF;
    END
    $$;

    CREATE TABLE IF NOT EXISTS audit_trail (
      id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
      sequence      bigserial   NOT NULL UNIQUE,
      timestamp     timestamptz NOT NULL DEFAULT now(),
      actor_type    text        NOT NULL CHECK (actor_type IN ('operator', 'agent', 'system')),
      actor_id      text        NOT NULL,
      acting_context jsonb,
      event_type    text        NOT NULL,
      payload       jsonb       NOT NULL,
      prev_hash     text,
      hash          text        NOT NULL
    );

    CREATE INDEX IF NOT EXISTS audit_trail_sequence_idx        ON audit_trail (sequence);
    CREATE INDEX IF NOT EXISTS audit_trail_actor_event_time_idx ON audit_trail (actor_id, event_type, timestamp);
  `);
};

exports.down = (pgm) => {
  // Restore old audit_trail if backup exists
  pgm.sql(`
    DO $$
    BEGIN
      DROP TABLE IF EXISTS audit_trail;
      IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'audit_trail_v1') THEN
        ALTER TABLE audit_trail_v1 RENAME TO audit_trail;
      END IF;
    END
    $$;
  `);
  // Remove added columns from schedules
  pgm.sql(`
    ALTER TABLE schedules
      DROP COLUMN IF EXISTS agent_instance_id,
      DROP COLUMN IF EXISTS agent_name,
      DROP COLUMN IF EXISTS type,
      DROP COLUMN IF EXISTS expression,
      DROP COLUMN IF EXISTS source,
      DROP COLUMN IF EXISTS next_run_at,
      DROP COLUMN IF EXISTS last_run_at,
      DROP COLUMN IF EXISTS last_run_status;
  `);
};
