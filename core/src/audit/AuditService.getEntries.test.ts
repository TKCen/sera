import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AuditService } from './AuditService.js';
import { pool } from '../lib/database.js';

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
    connect: vi.fn(),
  },
}));

describe('AuditService.getEntries', () => {
  let service: AuditService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = AuditService.getInstance();
  });

  it('uses QueryBuilder to construct queries correctly', async () => {
    vi.mocked(pool.query)
      .mockResolvedValueOnce({ rows: [{ count: '10' }] }) // count query
      .mockResolvedValueOnce({ rows: [] }); // entries query

    await service.getEntries({
      actorId: 'test-actor',
      eventType: 'test-event',
      limit: 20,
      offset: 5,
    });

    expect(pool.query).toHaveBeenCalledWith(
      expect.stringContaining(
        'SELECT COUNT(*) FROM audit_trail  WHERE actor_id = $1 AND event_type = $2'
      ),
      ['test-actor', 'test-event']
    );

    expect(pool.query).toHaveBeenCalledWith(
      expect.stringContaining(
        'SELECT * FROM audit_trail  WHERE actor_id = $1 AND event_type = $2 ORDER BY sequence DESC LIMIT $3 OFFSET $4'
      ),
      ['test-actor', 'test-event', 20, 5]
    );
  });

  it('handles empty filters', async () => {
    vi.mocked(pool.query)
      .mockResolvedValueOnce({ rows: [{ count: '0' }] })
      .mockResolvedValueOnce({ rows: [] });

    await service.getEntries({});

    expect(pool.query).toHaveBeenCalledWith(
      expect.stringContaining('SELECT COUNT(*) FROM audit_trail'),
      []
    );

    expect(pool.query).toHaveBeenCalledWith(
      expect.stringContaining('SELECT * FROM audit_trail'),
      [50, 0]
    );
    expect(pool.query).toHaveBeenCalledWith(
      expect.stringMatching(
        /SELECT \* FROM audit_trail\s+ORDER BY sequence DESC LIMIT \$1 OFFSET \$2/
      ),
      [50, 0]
    );
  });
});
