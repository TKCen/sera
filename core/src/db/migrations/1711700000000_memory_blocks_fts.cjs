/** @param pgm {import('node-pg-migrate').MigrationBuilder} */
exports.up = (pgm) => {
  pgm.createTable('memory_blocks', {
    id: { type: 'uuid', primaryKey: true },
    agent_id: { type: 'text' },
    namespace: { type: 'text', notNull: true },
    type: { type: 'text', notNull: true },
    title: { type: 'text' },
    content: { type: 'text', notNull: true },
    tags: { type: 'text[]', default: '{}' },
    importance: { type: 'integer', default: 3 },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  });

  pgm.sql("CREATE INDEX memory_blocks_fts_idx ON memory_blocks USING GIN (to_tsvector('english', coalesce(title, '') || ' ' || content))");
  pgm.createIndex('memory_blocks', ['namespace']);
  pgm.createIndex('memory_blocks', ['agent_id']);
};

/** @param pgm {import('node-pg-migrate').MigrationBuilder} */
exports.down = (pgm) => {
  pgm.dropTable('memory_blocks');
};
