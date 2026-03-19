/**
 * Migration: Epic 06 — Skill Packages
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */
exports.up = (pgm) => {
  pgm.createTable('skill_packages', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true },
    version: { type: 'text', notNull: true },
    description: { type: 'text' },
    skills: { type: 'jsonb', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  pgm.sql('CREATE UNIQUE INDEX IF NOT EXISTS skill_packages_name_version_idx ON skill_packages (name, version)');
};

exports.down = (pgm) => {
  pgm.dropTable('skill_packages');
};
