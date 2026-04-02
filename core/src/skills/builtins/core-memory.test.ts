import { describe, it, expect, vi, beforeEach } from 'vitest';
import { coreMemoryAppendSkill, coreMemoryReplaceSkill } from './core-memory.js';
import { CoreMemoryService } from '../../memory/CoreMemoryService.js';

vi.mock('../../memory/CoreMemoryService.js', () => {
  const mockService = {
    getBlockByName: vi.fn(),
    updateBlock: vi.fn(),
  };
  return {
    CoreMemoryService: {
      getInstance: vi.fn(() => mockService),
    },
  };
});

describe('Core Memory Skills', () => {
  const mockContext: any = { agentInstanceId: 'agent-1' };
  let mockService: any;

  beforeEach(() => {
    vi.clearAllMocks();
    mockService = CoreMemoryService.getInstance();
  });

  describe('core_memory_append', () => {
    it('should append content to a block', async () => {
      mockService.getBlockByName.mockResolvedValue({ content: 'Existing.' });
      mockService.updateBlock.mockResolvedValue({ content: 'Existing.\nNew.' });

      const result = await coreMemoryAppendSkill.handler(
        { block: 'persona', content: 'New.' },
        mockContext
      );

      if (!result.success) {
        console.error('Skill failed:', result.error);
      }
      expect(result.success).toBe(true);
      expect(mockService.updateBlock).toHaveBeenCalledWith('agent-1', 'persona', 'Existing.\nNew.');
    });

    it('should return error if block not found', async () => {
      mockService.getBlockByName.mockResolvedValue(null);

      const result = await coreMemoryAppendSkill.handler(
        { block: 'unknown', content: 'New.' },
        mockContext
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain('not found');
    });
  });

  describe('core_memory_replace', () => {
    it('should replace content in a block (global replacement)', async () => {
      mockService.getBlockByName.mockResolvedValue({ content: 'I like apples and apples.' });
      mockService.updateBlock.mockResolvedValue({ content: 'I like oranges and oranges.' });

      const result = await coreMemoryReplaceSkill.handler(
        { block: 'persona', old_content: 'apples', new_content: 'oranges' },
        mockContext
      );

      expect(result.success).toBe(true);
      expect(mockService.updateBlock).toHaveBeenCalledWith(
        'agent-1',
        'persona',
        'I like oranges and oranges.'
      );
    });

    it('should return error if old content not found', async () => {
      mockService.getBlockByName.mockResolvedValue({ content: 'I like apples.' });

      const result = await coreMemoryReplaceSkill.handler(
        { block: 'persona', old_content: 'bananas', new_content: 'oranges' },
        mockContext
      );

      expect(result.success).toBe(false);
      expect(result.error).toContain('not found in block');
    });
  });
});
