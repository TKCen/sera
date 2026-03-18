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
  });
  pgm.createIndex('api_keys', ['owner_sub']);
  pgm.createIndex('api_keys', ['key_hash']);

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
  });
  pgm.createIndex('secrets', ['name']);
};

exports.down = (pgm) => {
  pgm.dropTable('secrets');
  pgm.dropTable('api_keys');
};
