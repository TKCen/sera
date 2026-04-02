/**
 * Migration: Memory Blocks Full-Text Search
 *
 * Adds memory_blocks table for hybrid search indexing.
 * Synchronizes with Qdrant vector store and disk-based markdown blocks.
 */
exports.up = (pgm) => {
  pgm.createTable('memory_blocks', {
    id: { type: 'uuid', primaryKey: true },
    agent_id: { type: 'text', notNull: true },
    namespace: { type: 'text', notNull: true },
    type: { type: 'text', notNull: true },
    title: { type: 'text' },
    content: { type: 'text', notNull: true },
    tags: { type: 'text[]', default: '{}' },
    importance: { type: 'integer', default: 3 },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    metadata: { type: 'jsonb', default: '{}' },
    tsv: { type: 'tsvector' },
  });

  pgm.createIndex('memory_blocks', 'tsv', { method: 'gin' });
  pgm.createIndex('memory_blocks', ['agent_id', 'namespace']);
  pgm.createIndex('memory_blocks', 'created_at');

  // Trigger function to update tsv
  pgm.sql(`
    CREATE OR REPLACE FUNCTION memory_blocks_tsv_trigger() RETURNS trigger AS $$
    BEGIN
      new.tsv :=
        setweight(to_tsvector('english', coalesce(new.title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(new.content, '')), 'B');
      RETURN new;
    END
    $$ LANGUAGE plpgsql;
  `);

  pgm.sql(`
    CREATE TRIGGER tsvectorupdate BEFORE INSERT OR UPDATE
    ON memory_blocks FOR EACH ROW EXECUTE FUNCTION memory_blocks_tsv_trigger();
  `);
};

exports.down = (pgm) => {
  pgm.dropTable('memory_blocks');
  pgm.sql('DROP FUNCTION IF EXISTS memory_blocks_tsv_trigger() CASCADE');
};
