import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createKnowledgeStoreSkill } from './knowledge-store.js';
import { MemoryCategorizationService } from '../../memory/MemoryCategorizationService.js';
import { EmbeddingService } from '../../services/embedding.service.js';

vi.mock('../../memory/MemoryCategorizationService.js');
vi.mock('../../llm/LlmRouter.js', () => ({
  getLlmRouter: vi.fn(() => ({})),
}));

const mockResolve = vi.fn().mockReturnValue({ contextCompactionModel: 'cheap-model' });
vi.mock('../../llm/ProviderRegistry.js', () => ({
  ProviderRegistry: vi.fn().mockImplementation(function () {
    return {
      resolve: mockResolve,
    };
  }),
}));

const mockWrite = vi.fn().mockResolvedValue({
  id: 'mock-id',
  timestamp: new Date().toISOString(),
  title: 'mock-title',
  content: 'mock-content',
});
vi.mock('../../memory/blocks/ScopedMemoryBlockStore.js', () => ({
  ScopedMemoryBlockStore: vi.fn().mockImplementation(function () {
    return {
      write: mockWrite,
    };
  }),
}));

vi.mock('../../audit/AuditService.js', () => ({
  AuditService: {
    getInstance: vi.fn().mockReturnValue({
      record: vi.fn().mockResolvedValue({}),
    }),
  },
}));

vi.mock('../../services/vector.service.js', () => ({
  VectorService: vi.fn().mockImplementation(() => ({
    upsert: vi.fn().mockResolvedValue({}),
  })),
}));

describe('knowledge-store skill categorization', () => {
  const skill = createKnowledgeStoreSkill();
  const mockContext: any = {
    agentName: 'test-agent',
    manifest: {
      metadata: { name: 'test-agent' },
      model: { name: 'main-model' },
      memory: { categorize: true },
    },
  };

  beforeEach(() => {
    vi.spyOn(EmbeddingService.prototype, 'isAvailable').mockReturnValue(false);
  });

  it('should store multiple facts when categorization is enabled', async () => {
    const mockFacts = [
      {
        title: 'Fact 1',
        content: 'Content 1',
        type: 'fact',
        tags: ['tag1'],
        importance: 4,
      },
      {
        title: 'Fact 2',
        content: 'Content 2',
        type: 'insight',
        tags: ['tag2'],
        importance: 2,
      },
    ];

    vi.mocked(MemoryCategorizationService.categorize).mockResolvedValue(mockFacts);

    const params = {
      content: 'Multiple facts in one string',
      type: 'fact',
      scope: 'personal',
    };

    const result = await skill.handler(params, mockContext);

    expect(result.success).toBe(true);
    expect(result.data).toHaveProperty('blocks');
    expect(result.data.blocks).toHaveLength(2);
    expect(MemoryCategorizationService.categorize).toHaveBeenCalled();
    expect(mockWrite).toHaveBeenCalledTimes(2);
  });

  it('should store single fact when categorization is disabled', async () => {
    const contextWithoutCategorization = {
      ...mockContext,
      manifest: {
        ...mockContext.manifest,
        memory: { categorize: false },
      },
    };

    const params = {
      content: 'Single fact',
      type: 'fact',
      scope: 'personal',
    };

    mockWrite.mockClear();
    vi.mocked(MemoryCategorizationService.categorize).mockClear();

    const result = await skill.handler(params, contextWithoutCategorization);

    expect(result.success).toBe(true);
    expect(result.data).toHaveProperty('id');
    expect(result.data).not.toHaveProperty('blocks');
    expect(MemoryCategorizationService.categorize).not.toHaveBeenCalled();
    expect(mockWrite).toHaveBeenCalledTimes(1);
  });
});
