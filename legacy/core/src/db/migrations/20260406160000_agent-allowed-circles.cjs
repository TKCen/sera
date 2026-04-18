/**
 * Migration: Add allowed_circles to agent_instances
 *
 * Stores an explicit list of circle IDs (or names) an agent instance is
 * permitted to access. An empty array means no restriction beyond the
 * agent's own circle membership. NULL is treated identically to empty
 * (i.e. no whitelist enforced) for backward compatibility.
 */

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.up = (pgm) => {
  pgm.addColumn('agent_instances', {
    allowed_circles: {
      type: 'text[]',
      notNull: false,
      default: pgm.func("'{}'"),
    },
  });
};

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.down = (pgm) => {
  pgm.dropColumn('agent_instances', 'allowed_circles');
};
