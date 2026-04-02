/**
 * Migration: Add Error Type to Agent Command Log
 *
 * Adds error_type column to agent_command_log for distinguishing
 * between recoverable and fatal tool errors.
 */
exports.up = (pgm) => {
  pgm.addColumn('agent_command_log', {
    error_type: { type: 'text' },
  });
};

exports.down = (pgm) => {
  pgm.dropColumn('agent_command_log', 'error_type');
};
