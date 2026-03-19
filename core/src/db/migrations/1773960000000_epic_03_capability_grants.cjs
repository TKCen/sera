/**
 * Epic 03 — capability_grants table and agent_instances workspace quota columns.
 * Story 3.9 (capability_grants), Story 3.12 (workspace_used_gb, workspace_limit_gb)
 */

exports.up = (pgm) => {
  // Story 3.9 — runtime capability grants (one-time, session token, persistent)
  pgm.createTable('capability_grants', {
    id: {
      type: 'uuid',
      primaryKey: true,
      default: pgm.func('gen_random_uuid()'),
    },
    agent_instance_id: {
      type: 'uuid',
      notNull: true,
    },
    dimension: {
      type: 'varchar(64)',
      notNull: true,
      comment: 'filesystem | network | exec.commands',
    },
    value: {
      type: 'text',
      notNull: true,
      comment: 'The specific path, host, or command pattern granted',
    },
    grant_type: {
      type: 'varchar(16)',
      notNull: true,
      check: "grant_type IN ('one-time', 'session', 'persistent')",
    },
    granted_by: {
      type: 'text',
      notNull: false,
      comment: 'Operator identity (JWT sub) who approved the grant',
    },
    expires_at: {
      type: 'timestamptz',
      notNull: false,
    },
    revoked_at: {
      type: 'timestamptz',
      notNull: false,
    },
    created_at: {
      type: 'timestamptz',
      notNull: true,
      default: pgm.func('NOW()'),
    },
  });

  pgm.createIndex('capability_grants', ['agent_instance_id']);
  pgm.createIndex('capability_grants', ['agent_instance_id', 'revoked_at']);

  // Story 3.12 — workspace disk quota tracking on agent_instances
  pgm.addColumn('agent_instances', {
    workspace_used_gb: { type: 'numeric(10, 3)', notNull: false },
  });
};

exports.down = (pgm) => {
  pgm.dropColumn('agent_instances', 'workspace_used_gb');
  pgm.dropTable('capability_grants');
};
