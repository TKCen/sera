/**
 * Migration: Schedule Inner Life
 *
 * Adds `category` column to schedules table for classifying inner-life
 * schedule types (reflection, knowledge_consolidation, curiosity_research, etc.).
 *
 * Adds a partial index on task_queue for efficiently querying schedule-triggered
 * tasks via the context JSONB column.
 */

/** @type {import('node-pg-migrate').MigrationBuilder} */
exports.up = (pgm) => {
  pgm.addColumn('schedules', {
    category: { type: 'text', notNull: false },
  });

  pgm.createIndex('schedules', 'category', {
    name: 'schedules_category_idx',
    ifNotExists: true,
  });

  pgm.sql(`
    CREATE INDEX IF NOT EXISTS task_queue_schedule_ctx_idx
      ON task_queue ((context->'schedule'->>'scheduleId'))
      WHERE context->'schedule' IS NOT NULL
  `);
};

/** @type {import('node-pg-migrate').MigrationBuilder} */
exports.down = (pgm) => {
  pgm.sql('DROP INDEX IF EXISTS task_queue_schedule_ctx_idx');
  pgm.dropIndex('schedules', 'category', { name: 'schedules_category_idx', ifExists: true });
  pgm.dropColumn('schedules', 'category');
};
