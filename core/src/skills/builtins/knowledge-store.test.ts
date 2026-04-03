import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createKnowledgeStoreSkill } from './knowledge-store.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';

// Mock dependencies
const mockBlocks = new Map<string, Record<string, unknown>>();
const mockWrite = vi.fn().mockImplementation(async (opts: Record<string, unknown>) => {
  const block = {
    id: 'block-new',
    agentId: opts.agentId,
    type: opts.type,
    timestamp: new Date().toISOString(),
    tags: opts.tags ?? [],
    importance: opts.importance ?? 3,
    title: opts.title ?? 'Auto title',
    content: opts.content,
    ...(opts.sourceRef ? { sourceRef: opts.sourceRef } : {}),
  };
  mockBlocks.set(block.id, block);
  return block;
});
const mockFindBySourceRef = vi
  .fn()
  .mockImplementation(async (_agentId: string, sourceRef: Record<string, unknown>) => {
    for (const block of mockBlocks.values()) {
      if (
        block.sourceRef &&
        (block.sourceRef as Record<string, unknown>).scheduleId === sourceRef.scheduleId
      ) {
        return block;
      }
    }
    return null;
  });
const mockUpdate = vi
  .fn()
  .mockImplementation(async (_agentId: string, id: string, updates: Record<string, unknown>) => {
    const existing = mockBlocks.get(id);
    if (!existing) return null;
    const updated = { ...existing, ...updates };
    mockBlocks.set(id, updated);
    return updated;
  });

vi.mock('../../memory/blocks/ScopedMemoryBlockStore.js', () => {
  class MockScopedMemoryBlockStore {
    write = mockWrite;
    findBySourceRef = mockFindBySourceRef;
    update = mockUpdate;
  }
  return { ScopedMemoryBlockStore: MockScopedMemoryBlockStore };
});

vi.mock('../../services/embedding.service.js', () => ({
  EmbeddingService: {
    getInstance: () => ({ isAvailable: () => false }),
  },
}));

vi.mock('../../services/vector.service.js', () => ({
  VectorService: vi.fn(),
}));

vi.mock('../../memory/KnowledgeGitService.js', () => ({
  KnowledgeGitService: {
    getInstance: () => ({}),
  },
}));

vi.mock('../../audit/AuditService.js', () => ({
  AuditService: {
    getInstance: () => ({
      record: vi.fn().mockResolvedValue(undefined),
    }),
  },
}));

const mockContext: AgentContext = {
  agentName: 'TestAgent',
  workspacePath: '/tmp/test',
  tier: 1 as SecurityTier,
  manifest: {
    apiVersion: 'v1',
    kind: 'Agent',
    metadata: {
      name: 'TestAgent',
      displayName: 'Test Agent',
      icon: '',
      circle: 'test',
      tier: 1 as SecurityTier,
    },
    identity: { role: 'tester', description: 'Test agent' },
    model: { provider: 'openai', name: 'gpt-4' },
  },
  agentInstanceId: 'agent-001',
  containerId: 'container-001',
  sandboxManager: {} as never,
  sessionId: 'session-001',
};

describe('knowledge-store skill', () => {
  let skill: ReturnType<typeof createKnowledgeStoreSkill>;

  beforeEach(() => {
    vi.clearAllMocks();
    mockBlocks.clear();
    skill = createKnowledgeStoreSkill();
  });

  it('creates a block without sourceRef (backward compat)', async () => {
    const result = await skill.handler!(
      { content: 'Test content', type: 'fact', scope: 'personal' },
      mockContext
    );
    expect(result).toEqual(
      expect.objectContaining({
        success: true,
        data: expect.objectContaining({ success: true, updated: false }),
      })
    );
  });

  it('creates a block with sourceRef on first call', async () => {
    const result = await skill.handler!(
      {
        content: 'Daily summary',
        type: 'insight',
        scope: 'personal',
        sourceRef: { scheduleId: 'sched-001' },
        upsertMode: 'replace',
      },
      mockContext
    );
    expect(result).toEqual(
      expect.objectContaining({
        success: true,
      })
    );
  });

  it('rejects upsertMode=replace without sourceRef', async () => {
    const result = await skill.handler!(
      { content: 'Test', type: 'fact', scope: 'personal', upsertMode: 'replace' },
      mockContext
    );
    expect(result).toEqual(
      expect.objectContaining({
        success: false,
        error: expect.stringContaining('sourceRef'),
      })
    );
  });

  it('rejects upsertMode=replace with circle scope', async () => {
    const result = await skill.handler!(
      {
        content: 'Test',
        type: 'fact',
        scope: 'circle',
        sourceRef: { scheduleId: 'sched-001' },
        upsertMode: 'replace',
      },
      mockContext
    );
    expect(result).toEqual(
      expect.objectContaining({
        success: false,
        error: expect.stringContaining('personal scope'),
      })
    );
  });

  it('rejects upsertMode=replace with global scope', async () => {
    const result = await skill.handler!(
      {
        content: 'Test',
        type: 'fact',
        scope: 'global',
        sourceRef: { scheduleId: 'sched-001' },
        upsertMode: 'replace',
      },
      mockContext
    );
    expect(result).toEqual(
      expect.objectContaining({
        success: false,
        error: expect.stringContaining('personal scope'),
      })
    );
  });
});
