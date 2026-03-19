exports.up = (pgm) => {
  pgm.createTable('skills', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    skill_id: { type: 'text' }, // kebab-case identifier from front-matter
    name: { type: 'text', notNull: true },
    version: { type: 'text', notNull: true },
    description: { type: 'text', notNull: true },
    triggers: { type: 'jsonb', notNull: true, default: '[]' },
    requires: { type: 'jsonb', default: '[]' },
    conflicts: { type: 'jsonb', default: '[]' },
    max_tokens: { type: 'integer' },
    content: { type: 'text', notNull: true },
    source: { type: 'text', notNull: true, default: 'external' }, // 'bundled' | 'external'
    category: { type: 'text' },
    tags: { type: 'jsonb', default: '[]' },
    applies_to: { type: 'jsonb', default: '[]' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  });

  pgm.createIndex('skills', ['name', 'version'], { unique: true });
  pgm.createIndex('skills', 'skill_id');
};

exports.down = (pgm) => {
  pgm.dropTable('skills');
};
