/**
 * Migration: ADR-004 Permission Grant Persistence
 *
 * Move persistent permission grants from in-memory storage to PostgreSQL.
 */
exports.up = (pgm) => {
  pgm.createTable('permission_grants', {
    id: {
      type: 'uuid',
      primaryKey: true,
      default: pgm.func('gen_random_uuid()'),
    },
    agent_instance_id: {
      type: 'uuid',
      references: 'agent_instances(id)',
      onDelete: 'CASCADE',
    },
    grant_type: {
      type: 'text',
      notNull: true,
      check: "grant_type IN ('session', 'one-time', 'persistent')",
    },
    resource_type: {
      type: 'text',
      notNull: true,
    },
    resource_value: {
      type: 'text',
      notNull: true,
    },
    mode: {
      type: 'text',
      default: 'ro',
    },
    approved_by: {
      type: 'text',
    },
    created_at: {
      type: 'timestamptz',
      default: pgm.func('now()'),
    },
    expires_at: {
      type: 'timestamptz',
    },
    revoked_at: {
      type: 'timestamptz',
    },
  });

  pgm.createIndex('permission_grants', 'agent_instance_id');
  pgm.createIndex('permission_grants', ['resource_type', 'resource_value']);
};

exports.down = (pgm) => {
  pgm.dropTable('permission_grants');
};
