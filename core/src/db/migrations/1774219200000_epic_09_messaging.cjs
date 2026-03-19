exports.up = (pgm) => {
  // ── Thought Events (Story 9.7) ──────────────────────────────────────────
  pgm.createTable('thought_events', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    agent_instance_id: { type: 'uuid', notNull: true, references: 'agent_instances', onDelete: 'CASCADE' },
    task_id: { type: 'text' },
    step: { type: 'text', notNull: true },
    content: { type: 'text', notNull: true },
    iteration: { type: 'int', notNull: true, default: 0 },
    published_at: { type: 'timestamptz', notNull: true, default: pgm.func('now()') },
  });

  pgm.createIndex('thought_events', ['agent_instance_id', 'published_at']);
  pgm.createIndex('thought_events', ['task_id']);
};

exports.down = (pgm) => {
  pgm.dropTable('thought_events');
};
