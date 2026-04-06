import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createKnowledgeUpdateSkill } from './knowledge-update.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';

// ── Mocks ─────────────────────────────────────────────────────────────────────

const mockBlock = {
  id: 'block-001',
  agentId: 'agent-001',
  type: 'fact',
  timestamp: '2024-01-01T00:00:00.000Z',
  tags: ['test'],
  importance: 3,
  title: 'Test block',
  content: 'The quick brown fox jumps over the lazy dog.',
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

describe('knowledge-update skill', () => {
  let skill: ReturnType<typeof createKnowledgeUpdateSkill>;

  beforeEach(() => {
    vi.clearAllMocks();
    skill = createKnowledgeUpdateSkill();
  });

  it('replaces matching text in block content', async () => {
    mockReadByAgent.mockResolvedValueOnce(mockBlock);
    mockUpdate.mockResolvedValueOnce({
      ...mockBlock,
      content: 'The quick brown fox jumps over the energetic dog.',
    });

    const result = await skill.handler!(
      { blockId: 'block-001', oldText: 'lazy dog', newText: 'energetic dog' },
      mockContext
    );

    expect(result.success).toBe(true);
    expect(result.data).toEqual({ id: 'block-001', scope: 'personal', success: true });
    expect(mockUpdate).toHaveBeenCalledWith('agent-001', 'block-001', {
      content: 'The quick brown fox jumps over the energetic dog.',
    });
  });

  it('returns error when block is not found', async () => {
    mockReadByAgent.mockResolvedValueOnce(null);

    const result = await skill.handler!(
      { blockId: 'nonexistent', oldText: 'foo', newText: 'bar' },
      mockContext
    );

    expect(result.success).toBe(false);
    expect(result.error).toContain('nonexistent');
  });

  it('returns error when oldText is not found in content', async () => {
    mockReadByAgent.mockResolvedValueOnce(mockBlock);

    const result = await skill.handler!(
      { blockId: 'block-001', oldText: 'not present text', newText: 'replacement' },
      mockContext
    );

    expect(result.success).toBe(false);
    expect(result.error).toContain('not found in block');
  });

  it('returns error when blockId is missing', async () => {
    const result = await skill.handler!({ oldText: 'foo', newText: 'bar' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"blockId"');
  });

  it('returns error when oldText is missing', async () => {
    const result = await skill.handler!({ blockId: 'block-001', newText: 'bar' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"oldText"');
  });

  it('returns error when newText is missing', async () => {
    const result = await skill.handler!({ blockId: 'block-001', oldText: 'foo' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"newText"');
  });

  it('replaces only the first occurrence', async () => {
    const blockWithRepeat = {
      ...mockBlock,
      content: 'foo bar foo bar',
    };
    mockReadByAgent.mockResolvedValueOnce(blockWithRepeat);
    mockUpdate.mockResolvedValueOnce({ ...blockWithRepeat, content: 'baz bar foo bar' });

    await skill.handler!({ blockId: 'block-001', oldText: 'foo', newText: 'baz' }, mockContext);

    expect(mockUpdate).toHaveBeenCalledWith('agent-001', 'block-001', {
      content: 'baz bar foo bar',
    });
  });
});
