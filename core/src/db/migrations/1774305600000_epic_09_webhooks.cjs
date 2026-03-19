/**
 * Migration: Epic 09 — Webhooks
 * Story 9.8: webhooks and webhook_deliveries tables.
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */
exports.up = (pgm) => {
  pgm.createTable('webhooks', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true },
    url_path: { type: 'text', notNull: true, unique: true },
    secret: { type: 'text', notNull: true },
    event_type: { type: 'text', notNull: true },
    enabled: { type: 'boolean', notNull: true, default: true },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  }, { ifNotExists: true });

  pgm.createTable('webhook_deliveries', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    webhook_id: { type: 'uuid', notNull: true, references: 'webhooks', onDelete: 'CASCADE' },
    payload: { type: 'jsonb', notNull: true },
    status: { type: 'text', notNull: true },
    error_message: { type: 'text' },
    processed_at: { type: 'timestamptz' },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  }, { ifNotExists: true });

  pgm.sql(`
    CREATE INDEX IF NOT EXISTS webhooks_url_path_idx              ON webhooks (url_path);
    CREATE INDEX IF NOT EXISTS webhook_deliveries_wh_created_idx  ON webhook_deliveries (webhook_id, created_at);
  `);
};

exports.down = (pgm) => {
  pgm.dropTable('webhook_deliveries');
  pgm.dropTable('webhooks');
};
