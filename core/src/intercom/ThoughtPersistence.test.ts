import { describe, it, expect, vi, beforeEach } from 'vitest';
import { IntercomService } from './IntercomService.js';
import { pool } from '../lib/database.js';

// ── Mocks ───────────────────────────────────────────────────────────────────────

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn().mockResolvedValue({ rows: [] }),
  },
  query: vi.fn().mockResolvedValue({ rows: [] }),
}));

vi.mock('axios', () => ({
  default: {
    create: vi.fn(() => ({
      post: vi.fn().mockResolvedValue({ data: { result: {} } }),
    })),
  },
}));

// ── Tests ───────────────────────────────────────────────────────────────────────

describe('Story 9.7: Thought Stream Persistence', () => {
  let intercom: IntercomService;

  beforeEach(() => {
    vi.clearAllMocks();
    intercom = new IntercomService();
  });

  it('persists a thought to the database when published', async () => {
    const agentId = '11111111-2222-3333-4444-555555555555';
    const taskId = 'task-123';

    // Simulate non-blocking persistence
    await intercom.publishThought(agentId, 'Test Agent', 'reasoning', 'Thinking...', taskId);

    // Wait a tiny bit for the "fire and forget" async call
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(pool.query).toHaveBeenCalledWith(
      expect.stringContaining('INSERT INTO thought_events'),
      expect.arrayContaining([agentId, taskId, 'reasoning', 'Thinking...'])
    );
  });

  it('retrieves persisted thoughts with filtering', async () => {
    const agentId = '11111111-2222-3333-4444-555555555555';
    const taskId = 'task-123';

    const mockRows = [
      {
        published_at: new Date().toISOString(),
        step: 'plan',
        content: 'I have a plan',
        circle_id: 'test-circle', // Assuming a default value for circle_id to make it syntactically correct
        capabilities: [], // Assuming an empty array for capabilities to make it syntactically correct
        agent_instance_id: agentId,
        task_id: taskId,
        iteration: 1,
      },
    ];
    (pool.query as unknown as import('vitest').Mock).mockResolvedValue({
      rows: mockRows,
    });

    const thoughts = await intercom.getThoughts(agentId, { taskId });

    expect(pool.query).toHaveBeenCalledWith(
      expect.stringContaining(
        'SELECT * FROM thought_events WHERE agent_instance_id = $1 AND task_id = $2'
      ),
      expect.arrayContaining([agentId, taskId])
    );
    expect(thoughts).toHaveLength(1);
    expect(thoughts[0]!.content).toBe('I have a plan');
  });
});
