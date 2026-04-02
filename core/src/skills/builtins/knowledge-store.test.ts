import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createKnowledgeStoreSkill } from './knowledge-store.js';
import { ScopedMemoryBlockStore } from '../../memory/blocks/ScopedMemoryBlockStore.js';
import { MemoryAnalyst } from '../../memory/MemoryAnalyst.js';
import { EmbeddingService } from '../../services/embedding.service.js';

vi.mock('../../memory/blocks/ScopedMemoryBlockStore.js');
vi.mock('../../memory/MemoryAnalyst.js');
vi.mock('../../services/embedding.service.js');
vi.mock('../../audit/AuditService.js', () => ({
  AuditService: {
    getInstance: () => ({
      record: vi.fn().mockResolvedValue({}),
    }),
  },
}));

describe('knowledge-store skill', () => {
  const skill = createKnowledgeStoreSkill();

  beforeEach(() => {
    vi.clearAllMocks();

    // Mock EmbeddingService.getInstance()
    const mockEmbeddingService = {
      isAvailable: vi.fn().mockReturnValue(false),
      embed: vi.fn(),
    };
    vi.spyOn(EmbeddingService, 'getInstance').mockReturnValue(mockEmbeddingService as any);
  });

  it('should store a simple entry when analyzeOnSave is off', async () => {
    const mockContext = {
      agentInstanceId: 'agent-123',
      agentName: 'test-agent',
      manifest: { memory: { analyzeOnSave: false }, model: { name: 'gpt-4o' } },
    };
    const params = { content: 'test knowledge', type: 'fact', scope: 'personal' };

    (ScopedMemoryBlockStore.prototype.write as any) = vi.fn().mockResolvedValue({
      id: 'block-123',
      title: 'test knowledge',
      content: 'test knowledge',
      type: 'fact',
      timestamp: new Date().toISOString(),
      tags: [],
      importance: 3,
      agentId: 'agent-123',
    });

    const result = await skill.handler(params, mockContext as any);

    expect(result.success).toBe(true);
    expect(result.data.id).toBe('block-123');
    expect(MemoryAnalyst).not.toHaveBeenCalled();
  });

  it('should analyze and split entries when analyzeOnSave is on', async () => {
    const mockRouter = {
      getRegistry: () => ({
        resolve: () => ({ contextCompactionModel: 'cheap-model' }),
      }),
    };
    const mockContext = {
      agentInstanceId: 'agent-123',
      agentName: 'test-agent',
      manifest: { memory: { analyzeOnSave: true }, model: { name: 'gpt-4o' } },
      router: mockRouter,
    };
    const params = { content: 'complex content', type: 'fact', scope: 'personal' };

    (MemoryAnalyst.prototype.analyze as any) = vi.fn().mockResolvedValue({
      facts: [
        { title: 'Fact 1', content: 'content 1', importance: 4, tags: ['tag1'], scope: 'circle' },
        { title: 'Fact 2', content: 'content 2', importance: 2, tags: ['tag2'], scope: 'personal' },
      ],
    });

    (ScopedMemoryBlockStore.prototype.write as any) = vi.fn().mockImplementation(
      async (opts) =>
        ({
          id: `block-${opts.content === 'content 1' ? '1' : '2'}`,
          ...opts,
          timestamp: new Date().toISOString(),
        }) as any
    );

    const result = await skill.handler(params, mockContext as any);

    expect(result.success).toBe(true);
    expect(result.data.count).toBe(2);
    expect(result.data.ids).toEqual(['block-1', 'block-2']);
    expect(MemoryAnalyst).toHaveBeenCalled();
  });
});
