/**
 * Migration: Audit trail immutability
 *
 * Adds DB-level triggers that prevent UPDATE and DELETE on audit_trail rows.
 * Audit records must be append-only; modification would break the Merkle
 * hash-chain integrity guarantee.
 */

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.up = (pgm) => {
  pgm.sql(`
    CREATE OR REPLACE FUNCTION prevent_audit_modification()
    RETURNS TRIGGER AS $$
    BEGIN
      RAISE EXCEPTION 'audit_trail records are immutable — UPDATE and DELETE are not permitted';
    END;
    $$ LANGUAGE plpgsql;
  `);

  pgm.sql(`
    CREATE TRIGGER audit_trail_no_update
      BEFORE UPDATE ON audit_trail
      FOR EACH ROW
      EXECUTE FUNCTION prevent_audit_modification();
  `);

  pgm.sql(`
    CREATE TRIGGER audit_trail_no_delete
      BEFORE DELETE ON audit_trail
      FOR EACH ROW
      EXECUTE FUNCTION prevent_audit_modification();
  `);
};

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.down = (pgm) => {
  pgm.sql(`DROP TRIGGER IF EXISTS audit_trail_no_delete ON audit_trail;`);
  pgm.sql(`DROP TRIGGER IF EXISTS audit_trail_no_update ON audit_trail;`);
  pgm.sql(`DROP FUNCTION IF EXISTS prevent_audit_modification();`);
};
