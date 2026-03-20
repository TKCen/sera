/**
 * Migration: Epic 17 — Agent Identity & Delegation
 *
 * Adds agent_service_identities and delegation_tokens tables.
 */
exports.up = (pgm) => {
  // ── Agent Service Identities (Story 17.2) ───────────────────────────────
  pgm.createTable('agent_service_identities', {
    id: { type: 'uuid', primaryKey: true },
    agent_scope: { type: 'text', notNull: true }, // instance UUID, template name, or '*'
    service: { type: 'text', notNull: true },      // e.g. 'github', 'slack'
    external_id: { type: 'text' },                 // e.g. GitHub bot user login
    display_name: { type: 'text' },
    credential_secret_name: { type: 'text', notNull: true }, // ref to secrets table
    scopes: { type: 'text[]' },
    created_at: { type: 'timestamptz', default: pgm.func('now()') },
    rotated_at: { type: 'timestamptz' },
    expires_at: { type: 'timestamptz' },
    revoked_at: { type: 'timestamptz' },
  }, { ifNotExists: true });

  pgm.addIndex('agent_service_identities', ['agent_scope', 'service'], { ifNotExists: true });

  // ── Delegation Tokens (Story 17.3) ──────────────────────────────────────
  pgm.createTable('delegation_tokens', {
    id: { type: 'uuid', primaryKey: true },
    principal_type: { type: 'text', notNull: true },        // 'operator'
    principal_id: { type: 'text', notNull: true },          // operatorSub
    principal_name: { type: 'text', notNull: true },        // email
    actor_agent_id: { type: 'text', notNull: true },        // agentId (template or instance)
    actor_instance_id: { type: 'uuid' },                    // set if instance-scoped
    scope: { type: 'jsonb', notNull: true },                // DelegationScope
    grant_type: { type: 'text', notNull: true },            // 'one-time'|'session'|'persistent'
    credential_secret_name: { type: 'text', notNull: true },// secret being delegated
    signed_token: { type: 'text' },                         // signed JWT
    issued_at: { type: 'timestamptz', default: pgm.func('now()') },
    expires_at: { type: 'timestamptz' },
    revoked_at: { type: 'timestamptz' },
    last_used_at: { type: 'timestamptz' },
    use_count: { type: 'integer', default: 0 },
    parent_delegation_id: { type: 'uuid' }, // FK to delegation_tokens(id) — self-ref
  }, { ifNotExists: true });

  // Self-referential FK added separately to avoid chicken-and-egg
  pgm.sql(`
    ALTER TABLE delegation_tokens
    ADD CONSTRAINT delegation_tokens_parent_fk
    FOREIGN KEY (parent_delegation_id) REFERENCES delegation_tokens(id)
    ON DELETE SET NULL
  `);

  pgm.addIndex('delegation_tokens', 'actor_agent_id', { ifNotExists: true });
  pgm.addIndex('delegation_tokens', 'principal_id', { ifNotExists: true });
  pgm.addIndex('delegation_tokens', 'parent_delegation_id', { ifNotExists: true });
};

exports.down = (pgm) => {
  pgm.dropTable('delegation_tokens', { ifExists: true, cascade: true });
  pgm.dropTable('agent_service_identities', { ifExists: true, cascade: true });
};
