/** @param pgm {import('node-pg-migrate').MigrationBuilder} */
exports.up = (pgm) => {
  pgm.sql(`
    UPDATE agent_instances ai
    SET
      resolved_config = at.spec,
      template_applied_at = NOW()
    FROM agent_templates at
    WHERE ai.template_ref = at.name
      AND (ai.resolved_config IS NULL OR ai.template_applied_at IS NULL)
  `);
};

/** @param pgm {import('node-pg-migrate').MigrationBuilder} */
exports.down = (pgm) => {
  // No-op or potentially clearing these, but clearing might lose data if they were set intentionally.
  // Given the goal is a baseline, we'll leave it as a one-way migration for now.
};
