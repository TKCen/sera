import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CoreMemoryService } from './CoreMemoryService.js';
import { pool } from '../lib/database.js';

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
  },
}));

describe('CoreMemoryService', () => {
  let service: CoreMemoryService;

  beforeEach(() => {
    service = CoreMemoryService.getInstance();
    vi.clearAllMocks();
  });

  it('should fetch blocks for an agent', async () => {
    const mockRows = [{ id: '1', agent_id: 'agent-1', name: 'persona', content: 'test persona' }];
    vi.mocked(pool.query).mockResolvedValueOnce({ rows: mockRows } as any);

    const blocks = await service.getBlocks('agent-1');
    expect(blocks).toHaveLength(1);
    expect(blocks[0].name).toBe('persona');
    expect(pool.query).toHaveBeenCalledWith(expect.stringContaining('SELECT'), ['agent-1']);
  });

  it('should update a block', async () => {
    const mockBlock = {
      id: '1',
      agent_id: 'agent-1',
      name: 'persona',
      content: 'old',
      char_limit: 2000,
      is_readonly: false,
    };
    vi.mocked(pool.query).mockResolvedValueOnce({ rows: [mockBlock] } as any);
    vi.mocked(pool.query).mockResolvedValueOnce({
      rows: [{ ...mockBlock, content: 'new' }],
    } as any);

    const updated = await service.updateBlock('agent-1', 'persona', 'new');
    expect(updated.content).toBe('new');
    expect(pool.query).toHaveBeenCalledTimes(2);
  });

  it('should throw error when updating a read-only block', async () => {
    const mockBlock = {
      id: '1',
      agentId: 'agent-1',
      name: 'persona',
      content: 'old',
      charLimit: 2000,
      isReadonly: true,
    };
    vi.mocked(pool.query).mockResolvedValueOnce({ rows: [mockBlock] } as any);

    await expect(service.updateBlock('agent-1', 'persona', 'new')).rejects.toThrow(
      'Memory block "persona" is read-only'
    );
  });

  it('should throw error when content exceeds limit', async () => {
    const mockBlock = {
      id: '1',
      agentId: 'agent-1',
      name: 'persona',
      content: 'old',
      charLimit: 5,
      isReadonly: false,
    };
    vi.mocked(pool.query).mockResolvedValueOnce({ rows: [mockBlock] } as any);

    await expect(service.updateBlock('agent-1', 'persona', 'too long')).rejects.toThrow(
      'Content exceeds character limit'
    );
  });
});
