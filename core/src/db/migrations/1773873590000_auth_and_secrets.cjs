/**
 * Migration: Auth & Secrets
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */
exports.up = (pgm) => {
  // ── API Keys ────────────────────────────────────────────────────────────
  pgm.createTable('api_keys', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true },
    key_hash: { type: 'text', notNull: true },
    owner_sub: { type: 'text', notNull: true },
    roles: { type: 'text[]', notNull: true, default: '{}' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    expires_at: { type: 'timestamptz' },
    last_used_at: { type: 'timestamptz' },
    revoked_at: { type: 'timestamptz' },
  }, { ifNotExists: true });
  pgm.sql(`
    CREATE INDEX IF NOT EXISTS api_keys_owner_idx ON api_keys (owner_sub);
    CREATE INDEX IF NOT EXISTS api_keys_hash_idx  ON api_keys (key_hash);
  `);

  // ── Secrets ─────────────────────────────────────────────────────────────
  pgm.createTable('secrets', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true, unique: true },
    encrypted_value: { type: 'bytea', notNull: true },
    iv: { type: 'bytea', notNull: true },
    description: { type: 'text' },
    allowed_agents: { type: 'text[]', notNull: true, default: '{}' },
    tags: { type: 'text[]', notNull: true, default: '{}' },
    exposure: { type: 'text', notNull: true, default: 'per-call' },
    created_by: { type: 'text' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
    rotated_at: { type: 'timestamptz' },
    expires_at: { type: 'timestamptz' },
    deleted_at: { type: 'timestamptz' },
  }, { ifNotExists: true });
  pgm.sql('CREATE INDEX IF NOT EXISTS secrets_name_idx ON secrets (name)');
};

exports.down = (pgm) => {
  pgm.dropTable('secrets');
  pgm.dropTable('api_keys');
};
