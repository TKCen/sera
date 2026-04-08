/**
 * Migration: Add allowed_circles to secrets
 *
 * Stores a list of circle IDs that are permitted to access a secret.
 */

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.up = (pgm) => {
  pgm.addColumn('secrets', {
    allowed_circles: {
      type: 'text[]',
      notNull: true,
      default: pgm.func("'{}'"),
    },
  });
};

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.down = (pgm) => {
  pgm.dropColumn('secrets', 'allowed_circles');
};
