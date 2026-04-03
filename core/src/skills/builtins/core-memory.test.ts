import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createCoreMemoryAppendSkill, createCoreMemoryReplaceSkill } from './core-memory.js';
import { CoreMemoryService } from '../../memory/CoreMemoryService.js';
import { AuditService } from '../../audit/AuditService.js';
import type { Pool } from 'pg';

vi.mock('../../memory/CoreMemoryService.js');
vi.mock('../../audit/AuditService.js');

describe('Core Memory Skills', () => {
  const context = {
    agentInstanceId: 'agent-123',
    agentName: 'test-agent',
  };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(AuditService, 'getInstance').mockReturnValue({
      record: vi.fn().mockResolvedValue(undefined),
    } as any);
  });

  describe('core_memory_append', () => {
    it('appends content to a block', async () => {
      const skill = createCoreMemoryAppendSkill();
      const mockUpdate = { name: 'persona', content: 'new content', characterLimit: 2000 };
      const mockService = {
        appendBlock: vi.fn().mockResolvedValue(mockUpdate),
      };
      vi.spyOn(CoreMemoryService, 'getInstance').mockReturnValue(mockService as any);

      const result = (await skill.handler(
        { block: 'persona', content: 'more info' },
        context as any
      )) as { success: true; data: { content: string } };

      expect(result.success).toBe(true);
      expect(result.data.content).toBe('new content');
      expect(mockService.appendBlock).toHaveBeenCalledWith('agent-123', 'persona', 'more info');
    });

    it('returns error if append fails', async () => {
      const skill = createCoreMemoryAppendSkill();
      vi.spyOn(CoreMemoryService, 'getInstance').mockReturnValue({
        appendBlock: vi.fn().mockRejectedValue(new Error('Append failed')),
      } as any);

      const result = (await skill.handler(
        { block: 'persona', content: 'more info' },
        context as any
      )) as { success: false; error: string };

      expect(result.success).toBe(false);
      expect(result.error).toBe('Append failed');
    });
  });

  describe('core_memory_replace', () => {
    it('replaces text in a block', async () => {
      const skill = createCoreMemoryReplaceSkill();
      const mockUpdate = { name: 'persona', content: 'replaced content', characterLimit: 2000 };
      const mockService = {
        replaceInBlock: vi.fn().mockResolvedValue(mockUpdate),
      };
      vi.spyOn(CoreMemoryService, 'getInstance').mockReturnValue(mockService as any);

      const result = (await skill.handler(
        { block: 'persona', oldText: 'old', newText: 'new' },
        context as any
      )) as { success: true; data: { content: string } };

      expect(result.success).toBe(true);
      expect(result.data.content).toBe('replaced content');
      expect(mockService.replaceInBlock).toHaveBeenCalledWith('agent-123', 'persona', 'old', 'new');
    });
  });
});

function any() {
  return expect.anything();
}
