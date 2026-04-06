import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createKnowledgeRewriteSkill } from './knowledge-rewrite.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';

// ── Mocks ─────────────────────────────────────────────────────────────────────

const mockBlock = {
  id: 'block-001',
  agentId: 'agent-001',
  type: 'insight',
  timestamp: '2024-01-01T00:00:00.000Z',
  tags: ['old-tag'],
  importance: 3,
  title: 'Original title',
  content: 'Original content that will be completely replaced.',
};

const mockReadByAgent = vi.fn();
const mockUpdate = vi.fn();

vi.mock('../../memory/blocks/ScopedMemoryBlockStore.js', () => {
  class MockScopedMemoryBlockStore {
    readByAgent = mockReadByAgent;
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

vi.mock('../../audit/AuditService.js', () => ({
  AuditService: {
    getInstance: () => ({
      record: vi.fn().mockResolvedValue(undefined),
    }),
  },
}));

// ── Context ───────────────────────────────────────────────────────────────────

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

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('knowledge-rewrite skill', () => {
  let skill: ReturnType<typeof createKnowledgeRewriteSkill>;

  beforeEach(() => {
    vi.clearAllMocks();
    skill = createKnowledgeRewriteSkill();
  });

  it('rewrites block content without changing title', async () => {
    mockReadByAgent.mockResolvedValueOnce(mockBlock);
    mockUpdate.mockResolvedValueOnce({ ...mockBlock, content: 'Brand new consolidated content.' });

    const result = await skill.handler!(
      { blockId: 'block-001', newContent: 'Brand new consolidated content.' },
      mockContext
    );

    expect(result.success).toBe(true);
    expect(result.data).toEqual({ id: 'block-001', scope: 'personal', success: true });
    expect(mockUpdate).toHaveBeenCalledWith('agent-001', 'block-001', {
      content: 'Brand new consolidated content.',
    });
  });

  it('rewrites block content and updates title when newTitle is provided', async () => {
    mockReadByAgent.mockResolvedValueOnce(mockBlock);
    mockUpdate.mockResolvedValueOnce({
      ...mockBlock,
      content: 'New content.',
      title: 'Updated title',
    });

    const result = await skill.handler!(
      { blockId: 'block-001', newContent: 'New content.', newTitle: 'Updated title' },
      mockContext
    );

    expect(result.success).toBe(true);
    expect(mockUpdate).toHaveBeenCalledWith('agent-001', 'block-001', {
      content: 'New content.',
      title: 'Updated title',
    });
  });

  it('returns error when block is not found', async () => {
    mockReadByAgent.mockResolvedValueOnce(null);

    const result = await skill.handler!(
      { blockId: 'nonexistent', newContent: 'New content.' },
      mockContext
    );

    expect(result.success).toBe(false);
    expect(result.error).toContain('nonexistent');
    expect(mockUpdate).not.toHaveBeenCalled();
  });

  it('returns error when blockId is missing', async () => {
    const result = await skill.handler!({ newContent: 'New content.' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"blockId"');
  });

  it('returns error when newContent is missing', async () => {
    const result = await skill.handler!({ blockId: 'block-001' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"newContent"');
  });

  it('returns error when newContent is empty string', async () => {
    const result = await skill.handler!({ blockId: 'block-001', newContent: '   ' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"newContent"');
  });

  it('ignores blank newTitle (does not update title)', async () => {
    mockReadByAgent.mockResolvedValueOnce(mockBlock);
    mockUpdate.mockResolvedValueOnce({ ...mockBlock, content: 'New content.' });

    await skill.handler!(
      { blockId: 'block-001', newContent: 'New content.', newTitle: '   ' },
      mockContext
    );

    expect(mockUpdate).toHaveBeenCalledWith('agent-001', 'block-001', {
      content: 'New content.',
    });
  });
});
