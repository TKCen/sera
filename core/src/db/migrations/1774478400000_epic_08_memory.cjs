/**
 * Epic 08 — Memory & RAG
 * Creates the knowledge_merge_requests table for circle/global git-backed knowledge.
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.up = (pgm) => {
  pgm.createTable('knowledge_merge_requests', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    circle_id: { type: 'text', notNull: true },
    agent_instance_id: { type: 'text', notNull: true },
    agent_name: { type: 'text', notNull: true },
    branch: { type: 'text', notNull: true },
    status: {
      type: 'text',
      notNull: true,
      default: 'pending',
      comment: 'pending | approved | rejected | merged | conflict',
    },
    approved_by: { type: 'text' },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    diff_summary: { type: 'text' },
    conflict_details: { type: 'jsonb' },
  }, { ifNotExists: true });

  pgm.sql(`
    CREATE INDEX IF NOT EXISTS kmr_circle_status_idx ON knowledge_merge_requests (circle_id, status);
    CREATE INDEX IF NOT EXISTS kmr_agent_idx         ON knowledge_merge_requests (agent_instance_id);
  `);
};

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.down = (pgm) => {
  pgm.dropTable('knowledge_merge_requests');
};
