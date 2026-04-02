/**
 * Migration: Add detail column to thought_events
 */
exports.up = (pgm) => {
  pgm.addColumn(
    'thought_events',
    {
      detail: { type: 'jsonb' },
    },
    { ifNotExists: true }
  );
};

exports.down = (pgm) => {
  pgm.dropColumn('thought_events', 'detail');
};
