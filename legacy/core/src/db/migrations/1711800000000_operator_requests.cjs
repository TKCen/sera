/**
 * Migration: operator_requests table
 *
 * Stores requests raised by agent instances for operator review and action.
 */
exports.up = (pgm) => {
  pgm.createTable('operator_requests', {
    id: {
      type: 'uuid',
      primaryKey: true,
      default: pgm.func('gen_random_uuid()'),
    },
    agent_id: {
      type: 'text',
      notNull: true,
    },
    agent_name: {
      type: 'text',
    },
    type: {
      type: 'text',
      notNull: true,
    },
    title: {
      type: 'text',
      notNull: true,
    },
    payload: {
      type: 'jsonb',
      notNull: true,
      default: '{}',
    },
    status: {
      type: 'text',
      notNull: true,
      default: 'pending',
    },
    response: {
      type: 'jsonb',
    },
    created_at: {
      type: 'timestamptz',
      notNull: true,
      default: pgm.func('now()'),
    },
    resolved_at: {
      type: 'timestamptz',
    },
  });

  pgm.createIndex('operator_requests', 'status');
  pgm.createIndex('operator_requests', [{ name: 'created_at', sort: 'DESC' }]);
};

exports.down = (pgm) => {
  pgm.dropTable('operator_requests');
};
