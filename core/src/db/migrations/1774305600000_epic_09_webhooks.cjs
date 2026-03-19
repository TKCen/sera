exports.up = (pgm) => {
  // ── Webhooks (Story 9.8) ────────────────────────────────────────────────
  pgm.createTable('webhooks', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true },
    url_path: { type: 'text', notNull: true, unique: true }, // The slug used in /api/webhooks/incoming/:slug
    secret: { type: 'text', notNull: true }, // HMAC secret
    event_type: { type: 'text', notNull: true }, // e.g. 'external_alert' -> system.external_alert
    enabled: { type: 'boolean', notNull: true, default: true },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  });

  pgm.createTable('webhook_deliveries', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    webhook_id: { type: 'uuid', notNull: true, references: 'webhooks', onDelete: 'CASCADE' },
    payload: { type: 'jsonb', notNull: true },
    status: { type: 'text', notNull: true }, // pending, success, failed
    error_message: { type: 'text' },
    processed_at: { type: 'timestamptz' },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  });

  pgm.createIndex('webhooks', ['url_path']);
  pgm.createIndex('webhook_deliveries', ['webhook_id', 'created_at']);
};

exports.down = (pgm) => {
  pgm.dropTable('webhook_deliveries');
  pgm.dropTable('webhooks');
};
