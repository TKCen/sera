/**
 * Migration: Edit Channels and Routing Rules
 *
 * Adds description to notification_channels.
 * Adds enabled, priority, and target_agent_id to notification_routing_rules.
 */
exports.up = (pgm) => {
  pgm.addColumn('notification_channels', {
    description: { type: 'text' },
  });

  pgm.addColumns('notification_routing_rules', {
    enabled: { type: 'boolean', notNull: true, default: true },
    priority: { type: 'integer', notNull: true, default: 0 },
    target_agent_id: { type: 'text' },
  });

  pgm.addIndex('notification_routing_rules', 'priority');
  pgm.addIndex('notification_routing_rules', 'target_agent_id');
};

exports.down = (pgm) => {
  pgm.dropIndex('notification_routing_rules', 'target_agent_id');
  pgm.dropIndex('notification_routing_rules', 'priority');
  pgm.dropColumns('notification_routing_rules', ['enabled', 'priority', 'target_agent_id']);
  pgm.dropColumn('notification_channels', 'description');
};
