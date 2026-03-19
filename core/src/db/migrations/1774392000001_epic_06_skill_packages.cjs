exports.up = (pgm) => {
  pgm.createTable('skill_packages', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    name: { type: 'text', notNull: true },
    version: { type: 'text', notNull: true },
    description: { type: 'text' },
    skills: { type: 'jsonb', notNull: true }, // array of { name, version }
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', default: pgm.func('now()') },
  });

  pgm.createIndex('skill_packages', ['name', 'version'], { unique: true });
};

exports.down = (pgm) => {
  pgm.dropTable('skill_packages');
};
