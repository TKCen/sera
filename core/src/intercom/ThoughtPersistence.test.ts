import { describe, it, expect, vi, beforeEach } from 'vitest';
import { IntercomService } from './IntercomService.js';
import { pool } from '../lib/database.js';

// ── Mocks ───────────────────────────────────────────────────────────────────────

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn().mockResolvedValue({ rows: [] }),
  },
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
    const agentId = 'test-agent';
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
    const agentId = 'test-agent';
    const taskId = 'task-123';

    const mockRows = [
      {
        published_at: new Date().toISOString(),
        step: 'plan',
        content: 'I have a plan',
        agent_instance_id: agentId,
        task_id: taskId,
        iteration: 1,
      },
    ];
    (pool.query as any).mockResolvedValue({ rows: mockRows });

    const thoughts = await intercom.getThoughts(agentId, { taskId });

    expect(pool.query).toHaveBeenCalledWith(
      expect.stringContaining(
        'SELECT * FROM thought_events WHERE agent_instance_id = $1 AND task_id = $2'
      ),
      expect.arrayContaining([agentId, taskId])
    );
    expect(thoughts).toHaveLength(1);
    expect(thoughts[0].content).toBe('I have a plan');
  });
});
