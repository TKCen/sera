exports.up = (pgm) => {
  pgm.addColumn('named_lists', {
    always_enforced: { type: 'boolean', notNull: true, default: false }
  });
  pgm.addColumn('agent_instances', {
    last_heartbeat_at: { type: 'timestamptz' }
  });
  pgm.createIndex('named_lists', ['always_enforced']);
  pgm.createIndex('agent_instances', ['last_heartbeat_at']);
};

exports.down = (pgm) => {
  pgm.dropColumn('agent_instances', 'last_heartbeat_at');
  pgm.dropColumn('named_lists', 'always_enforced');
};
