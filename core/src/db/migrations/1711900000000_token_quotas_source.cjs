/**
 * Add `source` column to token_quotas to distinguish manifest-set vs operator-set budgets.
 * syncManifestBudget() will only overwrite rows where source='manifest', preserving operator overrides.
 */

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.up = (pgm) => {
  pgm.addColumn('token_quotas', {
    source: {
      type: 'text',
      notNull: true,
      default: 'manifest',
      check: "source IN ('manifest', 'operator')",
    },
  });
};

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.down = (pgm) => {
  pgm.dropColumn('token_quotas', 'source');
};
