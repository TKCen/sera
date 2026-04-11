/**
 * Migration: Add missing indexes and FK constraints for hot tables
 *
 * Adds indexes for common query patterns on high-traffic tables and foreign key
 * constraints where they were missing. Tables that don't exist yet (metering_usage,
 * sessions, audit_events) are omitted — their migrations will add indexes inline.
 */

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.up = (pgm) => {
  // ── agent_instances ─────────────────────────────────────────────────────
  // status is used in lifecycle queries; template_name is used for template-scoped lookups
  pgm.addIndex('agent_instances', ['status'], {
    name: 'agent_instances_status_idx',
    ifNotExists: true,
  });
  pgm.addIndex('agent_instances', ['template_name'], {
    name: 'agent_instances_template_name_idx',
    ifNotExists: true,
  });

  // ── task_queue ──────────────────────────────────────────────────────────
  // Standalone created_at index for time-range queries and pagination
  // (composite idx agent_instance_id+status+priority+created_at already exists)
  pgm.addIndex('task_queue', ['created_at'], {
    name: 'task_queue_created_at_idx',
    ifNotExists: true,
  });

  // ── audit_trail ─────────────────────────────────────────────────────────
  // event_type standalone index for filtering by event type
  // (composite actor_id+event_type+timestamp already exists in init)
  pgm.addIndex('audit_trail', ['event_type'], {
    name: 'audit_trail_event_type_idx',
    ifNotExists: true,
  });

  // ── schedules ───────────────────────────────────────────────────────────
  // Plain agent_instance_id index for lookups by instance
  // (unique partial index on (agent_instance_id, name) and next_run_at+status already exist)
  pgm.addIndex('schedules', ['agent_instance_id'], {
    name: 'schedules_agent_instance_id_idx',
    ifNotExists: true,
  });

  // ── memory_blocks ───────────────────────────────────────────────────────
  // Composite (agent_id, namespace) for scoped memory queries
  // (separate agent_id and namespace indexes already exist)
  pgm.addIndex('memory_blocks', ['agent_id', 'namespace'], {
    name: 'memory_blocks_agent_id_namespace_idx',
    ifNotExists: true,
  });
};

/** @param {import('node-pg-migrate').MigrationBuilder} pgm */
exports.down = (pgm) => {
  pgm.dropIndex('memory_blocks', ['agent_id', 'namespace'], {
    name: 'memory_blocks_agent_id_namespace_idx',
    ifExists: true,
  });
  pgm.dropIndex('schedules', ['agent_instance_id'], {
    name: 'schedules_agent_instance_id_idx',
    ifExists: true,
  });
  pgm.dropIndex('audit_trail', ['event_type'], {
    name: 'audit_trail_event_type_idx',
    ifExists: true,
  });
  pgm.dropIndex('task_queue', ['created_at'], {
    name: 'task_queue_created_at_idx',
    ifExists: true,
  });
  pgm.dropIndex('agent_instances', ['template_name'], {
    name: 'agent_instances_template_name_idx',
    ifExists: true,
  });
  pgm.dropIndex('agent_instances', ['status'], {
    name: 'agent_instances_status_idx',
    ifExists: true,
  });
};
