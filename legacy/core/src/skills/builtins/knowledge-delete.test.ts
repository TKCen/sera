import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createKnowledgeDeleteSkill } from './knowledge-delete.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';

// ── Mocks ─────────────────────────────────────────────────────────────────────

const mockBlock = {
  id: 'block-001',
  agentId: 'agent-001',
  type: 'fact',
  timestamp: '2024-01-01T00:00:00.000Z',
  tags: [],
  importance: 3,
  title: 'Test block',
  content: 'Some content to delete.',
};

const mockReadByAgent = vi.fn();
const mockDelete = vi.fn();

vi.mock('../../memory/blocks/ScopedMemoryBlockStore.js', () => {
  class MockScopedMemoryBlockStore {
    readByAgent = mockReadByAgent;
    delete = mockDelete;
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

describe('knowledge-delete skill', () => {
  let skill: ReturnType<typeof createKnowledgeDeleteSkill>;

  beforeEach(() => {
    vi.clearAllMocks();
    skill = createKnowledgeDeleteSkill();
  });

  it('deletes an existing block', async () => {
    mockReadByAgent.mockResolvedValueOnce(mockBlock);
    mockDelete.mockResolvedValueOnce(true);

    const result = await skill.handler!({ blockId: 'block-001' }, mockContext);

    expect(result.success).toBe(true);
    expect(result.data).toEqual({ id: 'block-001', scope: 'personal', deleted: true });
    expect(mockDelete).toHaveBeenCalledWith('agent-001', 'block-001');
  });

  it('returns error when block is not found', async () => {
    mockReadByAgent.mockResolvedValueOnce(null);

    const result = await skill.handler!({ blockId: 'nonexistent' }, mockContext);

    expect(result.success).toBe(false);
    expect(result.error).toContain('nonexistent');
    expect(mockDelete).not.toHaveBeenCalled();
  });

  it('returns error when store delete fails', async () => {
    mockReadByAgent.mockResolvedValueOnce(mockBlock);
    mockDelete.mockResolvedValueOnce(false);

    const result = await skill.handler!({ blockId: 'block-001' }, mockContext);

    expect(result.success).toBe(false);
    expect(result.error).toContain('Failed to delete');
  });

  it('returns error when blockId is missing', async () => {
    const result = await skill.handler!({}, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"blockId"');
  });

  it('returns error when blockId is empty string', async () => {
    const result = await skill.handler!({ blockId: '   ' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"blockId"');
  });
});
