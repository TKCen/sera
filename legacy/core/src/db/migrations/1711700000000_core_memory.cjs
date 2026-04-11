exports.up = (pgm) => {
  pgm.createTable('core_memory_blocks', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    agent_instance_id: {
      type: 'uuid',
      notNull: true,
      references: '"agent_instances"',
      onDelete: 'CASCADE',
    },
    name: { type: 'text', notNull: true },
    content: { type: 'text', notNull: true, default: '' },
    character_limit: { type: 'integer', notNull: true, default: 2000 },
    is_read_only: { type: 'boolean', notNull: true, default: false },
    created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    updated_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  });

  pgm.addConstraint('core_memory_blocks', 'core_memory_blocks_agent_instance_id_name_key', {
    unique: ['agent_instance_id', 'name'],
  });

  pgm.createIndex('core_memory_blocks', 'agent_instance_id');
};

exports.down = (pgm) => {
  pgm.dropTable('core_memory_blocks');
};
