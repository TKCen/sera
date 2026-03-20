/**
 * Migration: Epic 18 — Integration Channels
 *
 * Adds notification_channels, notification_routing_rules,
 * channel_user_mappings, and inbound_channel_routes tables.
 */
exports.up = (pgm) => {
  // ── Notification Channels (Story 18.1) ──────────────────────────────────
  pgm.createTable('notification_channels', {
    id: { type: 'uuid', primaryKey: true },
    name: { type: 'text', notNull: true },
    type: { type: 'text', notNull: true }, // 'webhook'|'email'|'discord'|'slack'
    config: { type: 'jsonb', notNull: true, default: '{}' }, // adapter config (sensitive fields redacted on reads)
    enabled: { type: 'boolean', notNull: true, default: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  pgm.addIndex('notification_channels', 'type', { ifNotExists: true });

  // ── Notification Routing Rules (Story 18.2) ─────────────────────────────
  pgm.createTable('notification_routing_rules', {
    id: { type: 'uuid', primaryKey: true },
    event_type: { type: 'text', notNull: true }, // exact type or wildcard, e.g. 'permission.*'
    channel_ids: { type: 'text[]', notNull: true, default: pgm.func("'{}'") },
    filter: { type: 'jsonb' },                  // optional field match conditions
    min_severity: { type: 'text', default: 'info' }, // 'info'|'warning'|'critical'
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  pgm.addIndex('notification_routing_rules', 'event_type', { ifNotExists: true });

  // ── Notification Dispatch Log — for dedup and retry tracking ────────────
  pgm.createTable('notification_dispatches', {
    id: { type: 'uuid', primaryKey: true },
    event_id: { type: 'text', notNull: true },
    channel_id: { type: 'uuid', notNull: true },
    event_type: { type: 'text', notNull: true },
    status: { type: 'text', notNull: true, default: 'pending' }, // 'pending'|'sent'|'failed'
    attempts: { type: 'integer', notNull: true, default: 0 },
    last_error: { type: 'text' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    sent_at: { type: 'timestamptz' },
  }, { ifNotExists: true });

  pgm.addIndex('notification_dispatches', ['event_id', 'channel_id'], { ifNotExists: true });

  // ── Channel User Mappings (Stories 18.4/18.5) ────────────────────────────
  pgm.createTable('channel_user_mappings', {
    id: { type: 'uuid', primaryKey: true },
    channel_id: { type: 'uuid', notNull: true },
    channel_type: { type: 'text', notNull: true }, // 'discord'|'slack'
    platform_user_id: { type: 'text', notNull: true },
    operator_sub: { type: 'text', notNull: true },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  pgm.addConstraint('channel_user_mappings', 'channel_user_mappings_unique',
    'UNIQUE (channel_type, platform_user_id)');

  // ── Inbound Channel Routes (Story 18.5) ─────────────────────────────────
  pgm.createTable('inbound_channel_routes', {
    id: { type: 'uuid', primaryKey: true },
    channel_id: { type: 'uuid', notNull: true },
    channel_type: { type: 'text', notNull: true },
    platform_channel_id: { type: 'text', notNull: true },
    target_agent_id: { type: 'text', notNull: true },
    prefix: { type: 'text' },
    task_template: { type: 'text', notNull: true, default: '{{message}}' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
  }, { ifNotExists: true });

  pgm.addIndex('inbound_channel_routes', ['channel_type', 'platform_channel_id'], { ifNotExists: true });
};

exports.down = (pgm) => {
  pgm.dropTable('inbound_channel_routes', { ifExists: true, cascade: true });
  pgm.dropTable('channel_user_mappings', { ifExists: true, cascade: true });
  pgm.dropTable('notification_dispatches', { ifExists: true, cascade: true });
  pgm.dropTable('notification_routing_rules', { ifExists: true, cascade: true });
  pgm.dropTable('notification_channels', { ifExists: true, cascade: true });
};
