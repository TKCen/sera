import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AgentScheduler } from './AgentScheduler.js';

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

import { query } from '../lib/database.js';
const mockQuery = query as unknown as import('vitest').Mock;

describe('AgentScheduler.isWithinQuota', () => {
  let scheduler: AgentScheduler;

  beforeEach(() => {
    vi.clearAllMocks();
    scheduler = new AgentScheduler();
  });

  it('returns true immediately when limit is 0 (unlimited) without hitting the DB (#748)', async () => {
    const result = await scheduler.isWithinQuota('agent-1', 0);

    expect(result).toBe(true);
    // Must not query the DB — 0 means unlimited, skip entirely
    expect(mockQuery).not.toHaveBeenCalled();
  });

  it('returns true when usage is below the limit', async () => {
    mockQuery.mockResolvedValueOnce({ rows: [{ total: '500' }] });

    const result = await scheduler.isWithinQuota('agent-1', 1000);

    expect(result).toBe(true);
  });

  it('returns false when usage meets or exceeds the limit', async () => {
    mockQuery.mockResolvedValueOnce({ rows: [{ total: '1000' }] });

    const result = await scheduler.isWithinQuota('agent-1', 1000);

    expect(result).toBe(false);
  });

  it('returns true (fail-open) when the DB query throws', async () => {
    mockQuery.mockRejectedValueOnce(new Error('DB connection lost'));

    const result = await scheduler.isWithinQuota('agent-1', 5000);

    expect(result).toBe(true);
  });

  it('returns true when usage row is missing (treats as 0 used)', async () => {
    mockQuery.mockResolvedValueOnce({ rows: [{ total: null }] });

    const result = await scheduler.isWithinQuota('agent-1', 100);

    expect(result).toBe(true);
  });
});
