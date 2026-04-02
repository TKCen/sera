import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CoreMemoryService } from './CoreMemoryService.js';
import type { Pool } from 'pg';

describe('CoreMemoryService', () => {
  let mockPool: any;
  let service: CoreMemoryService;
  const agentId = '00000000-0000-0000-0000-000000000001';

  beforeEach(() => {
    mockPool = {
      query: vi.fn(),
    };
    service = new CoreMemoryService(mockPool as unknown as Pool);
  });

  it('initializes default blocks', async () => {
    mockPool.query.mockResolvedValue({ rowCount: 1 });
    await service.initializeDefaultBlocks(agentId);
    expect(mockPool.query).toHaveBeenCalledTimes(2);
    expect(mockPool.query).toHaveBeenCalledWith(expect.stringContaining('INSERT INTO core_memory_blocks'), expect.arrayContaining([agentId, 'persona']));
    expect(mockPool.query).toHaveBeenCalledWith(expect.stringContaining('INSERT INTO core_memory_blocks'), expect.arrayContaining([agentId, 'human']));
  });

  it('updates a block', async () => {
    const mockBlock = { id: '1', name: 'persona', content: 'Updated Persona', characterLimit: 2000, isReadOnly: false };
    mockPool.query.mockResolvedValue({ rowCount: 1, rows: [mockBlock] });

    const updated = await service.updateBlock(agentId, 'persona', { content: 'Updated Persona' });
    expect(updated.content).toBe('Updated Persona');
    expect(mockPool.query).toHaveBeenCalledWith(expect.stringContaining('UPDATE core_memory_blocks'), expect.arrayContaining([agentId, 'persona', 'Updated Persona']));
  });

  it('appends to a block', async () => {
    const existingBlock = { id: '1', name: 'persona', content: 'Line 1', characterLimit: 2000, isReadOnly: false };
    const updatedBlock = { ...existingBlock, content: 'Line 1\nLine 2' };

    mockPool.query
      .mockResolvedValueOnce({ rows: [existingBlock] }) // getBlock
      .mockResolvedValueOnce({ rowCount: 1, rows: [updatedBlock] }); // updateBlock

    const updated = await service.appendBlock(agentId, 'persona', 'Line 2');
    expect(updated.content).toBe('Line 1\nLine 2');
  });

  it('replaces text in a block', async () => {
    const existingBlock = { id: '1', name: 'persona', content: 'The quick brown fox', characterLimit: 2000, isReadOnly: false };
    const updatedBlock = { ...existingBlock, content: 'The quick red fox' };

    mockPool.query
      .mockResolvedValueOnce({ rows: [existingBlock] }) // getBlock
      .mockResolvedValueOnce({ rowCount: 1, rows: [updatedBlock] }); // updateBlock

    const updated = await service.replaceInBlock(agentId, 'persona', 'brown', 'red');
    expect(updated.content).toBe('The quick red fox');
  });

  it('enforces character limits on append', async () => {
    const existingBlock = { id: '1', name: 'persona', content: 'A', characterLimit: 2, isReadOnly: false };
    mockPool.query.mockResolvedValueOnce({ rows: [existingBlock] });

    await expect(service.appendBlock(agentId, 'persona', 'Too long'))
      .rejects.toThrow(/exceeds character limit/);
  });

  it('enforces read-only status', async () => {
    const existingBlock = { id: '1', name: 'persona', content: 'Fixed', characterLimit: 2000, isReadOnly: true };
    mockPool.query.mockResolvedValueOnce({ rows: [existingBlock] });

    await expect(service.appendBlock(agentId, 'persona', 'More text'))
      .rejects.toThrow(/is read-only/);
  });
});
