/**
 * Migration: Agent Command Log
 *
 * Adds table for granular tool invocation logging for debugging.
 */
exports.up = (pgm) => {
  pgm.createTable(
    'agent_command_log',
    {
      id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
      session_id: { type: 'uuid', notNull: true, references: 'chat_sessions', onDelete: 'CASCADE' },
      agent_instance_id: {
        type: 'uuid',
        notNull: true,
        references: 'agent_instances',
        onDelete: 'CASCADE',
      },
      tool_name: { type: 'text', notNull: true },
      arguments: { type: 'jsonb', notNull: true },
      result: { type: 'text' },
      duration_ms: { type: 'int' },
      status: { type: 'text', notNull: true },
      created_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
    },
    { ifNotExists: true }
  );

  pgm.createIndex('agent_command_log', 'session_id', { ifNotExists: true });
  pgm.createIndex('agent_command_log', 'agent_instance_id', { ifNotExists: true });
};

exports.down = (pgm) => {
  pgm.dropTable('agent_command_log', { ifExists: true });
};
