/**
 * Migration: Agent Core Memory
 *
 * Stores named memory blocks (persona, human, context) for each agent instance.
 * These blocks are injected directly into the system prompt as editable text.
 */
exports.up = (pgm) => {
  pgm.createTable(
    'agent_core_memory',
    {
      id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
      agent_id: {
        type: 'uuid',
        notNull: true,
        references: 'agent_instances',
        onDelete: 'CASCADE',
      },
      name: { type: 'text', notNull: true },
      content: { type: 'text', notNull: true, default: '' },
      char_limit: { type: 'integer', notNull: true, default: 2000 },
      is_readonly: { type: 'boolean', notNull: true, default: false },
      created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
      updated_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    },
    { ifNotExists: true }
  );

  pgm.createIndex('agent_core_memory', 'agent_id');
  pgm.addConstraint('agent_core_memory', 'agent_core_memory_agent_name_unique', {
    unique: ['agent_id', 'name'],
  });
};

exports.down = (pgm) => {
  pgm.dropTable('agent_core_memory');
};
