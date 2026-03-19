/**
 * Epic 16 — Authentication & Secrets: grant identity columns.
 * Story 16.10: capability_grants.granted_by_email + granted_by_name so the
 * full OperatorIdentity (sub, email, name) is persisted on every grant.
 * All DDL is idempotent (IF NOT EXISTS / ADD COLUMN IF NOT EXISTS).
 */

exports.up = (pgm) => {
  pgm.sql(`
    ALTER TABLE capability_grants
      ADD COLUMN IF NOT EXISTS granted_by_email TEXT,
      ADD COLUMN IF NOT EXISTS granted_by_name  TEXT;
  `);
};

exports.down = (pgm) => {
  pgm.sql(`
    ALTER TABLE capability_grants
      DROP COLUMN IF EXISTS granted_by_email,
      DROP COLUMN IF EXISTS granted_by_name;
  `);
};
