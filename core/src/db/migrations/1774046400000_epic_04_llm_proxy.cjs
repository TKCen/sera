/**
 * Migration: Epic 04 — LLM Proxy & Governance
 *
 * Adds columns to usage_events required by Story 4.4 (metering):
 *   - cost_usd: cost estimate for the call (populated when provider returns pricing)
 *   - latency_ms: end-to-end proxy latency
 *   - status: 'success' | 'error'
 *
 * Also adds an index on (agent_id, created_at) for budget window queries.
 */

exports.up = (pgm) => {
  // Extend usage_events with metering columns
  pgm.addColumns('usage_events', {
    cost_usd: {
      type: 'numeric(10,6)',
      notNull: false,
    },
    latency_ms: {
      type: 'int',
      notNull: false,
    },
    status: {
      type: 'text',
      notNull: true,
      default: 'success',
    },
  });

  // Index for budget window queries: covering index on agent_id + time window
  pgm.createIndex('usage_events', ['agent_id', { name: 'created_at', sort: 'DESC' }], {
    name: 'usage_events_agent_time_idx',
    ifNotExists: true,
  });
};

exports.down = (pgm) => {
  pgm.dropIndex('usage_events', [], { name: 'usage_events_agent_time_idx', ifExists: true });
  pgm.dropColumns('usage_events', ['cost_usd', 'latency_ms', 'status']);
};
