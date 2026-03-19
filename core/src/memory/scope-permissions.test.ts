/**
 * Unit tests for knowledge scope permission checks.
 * Validates all three scopes (personal, circle, global) — positive and negative cases.
 */

import { describe, it, expect, vi } from 'vitest';

// Mock all I/O so these run fast and offline
vi.mock('../services/embedding.service.js', () => ({
  EmbeddingService: {
    getInstance: () => ({
      isAvailable: () => true,
      embed: vi.fn().mockResolvedValue(new Array(768).fill(0.1)),
    }),
  },
  EMBEDDING_VECTOR_SIZE: 768,
}));

vi.mock('../services/vector.service.js', () => ({
  VectorService: class {
    upsert = vi.fn().mockResolvedValue(undefined);
    search = vi.fn().mockResolvedValue([]);
    delete = vi.fn().mockResolvedValue(undefined);
    rebuildNamespace = vi.fn().mockResolvedValue(undefined);
    getCollectionInfo = vi.fn().mockResolvedValue({ vectorCount: 0 });
    searchLegacy = vi.fn().mockResolvedValue([]);
    deletePoints = vi.fn().mockResolvedValue(undefined);
    upsertPoints = vi.fn().mockResolvedValue(undefined);
    ensureCollection = vi.fn().mockResolvedValue(undefined);
  },
  collectionName: (ns: string) => ns,
}));

vi.mock('./blocks/ScopedMemoryBlockStore.js', () => ({
  ScopedMemoryBlockStore: class {
    write = vi.fn().mockResolvedValue({
      id: 'test-id',
      agentId: 'agent-1',
      type: 'fact',
      timestamp: new Date().toISOString(),
      tags: [],
      importance: 3,
      title: 'Test',
      content: 'Test content',
    });
  },
}));

vi.mock('./KnowledgeGitService.js', () => ({
  KnowledgeGitService: {
    getInstance: () => ({
      write: vi.fn().mockResolvedValue({
        block: { id: 'git-id', agentId: 'a', type: 'fact', timestamp: new Date().toISOString(), tags: [], importance: 3, title: 'T', content: 'C' },
        commitHash: 'abc123',
      }),
      autoMerge: vi.fn().mockResolvedValue(undefined),
      createMergeRequest: vi.fn().mockResolvedValue({ id: 'mr-1', status: 'pending' }),
    }),
  },
}));

vi.mock('../audit/AuditService.js', () => ({
  AuditService: { getInstance: () => ({ record: vi.fn().mockResolvedValue(undefined) }) },
}));

import { createKnowledgeStoreSkill } from '../skills/builtins/knowledge-store.js';
import { createKnowledgeQuerySkill } from '../skills/builtins/knowledge-query.js';
import type { AgentContext } from '../skills/types.js';

function makeContext(overrides: Partial<{
  agentInstanceId: string;
  capabilities: string[];
  circle: string;
  additionalCircles: string[];
}>): AgentContext {
  return {
    agentName: 'TestAgent',
    workspacePath: '/tmp/ws',
    tier: 2,
    agentInstanceId: overrides.agentInstanceId ?? 'instance-1',
    containerId: undefined,
    sessionId: 'sess-1',
    sandboxManager: undefined,
    manifest: {
      apiVersion: 'sera/v1',
      kind: 'Agent',
      metadata: {
        name: 'TestAgent',
        displayName: 'Test Agent',
        icon: '🤖',
        circle: overrides.circle ?? '',
        ...(overrides.additionalCircles ? { additionalCircles: overrides.additionalCircles } : {}),
        tier: 2,
      },
      identity: { role: 'test', description: '' },
      model: { provider: 'test', name: 'test' },
      capabilities: overrides.capabilities ?? [],
      memory: { personalMemory: 'enabled' },
    },
  };
}

describe('knowledge-store scope permission checks', () => {
  const storeSkill = createKnowledgeStoreSkill();

  const baseParams = {
    content: 'Test knowledge',
    type: 'fact',
  };

  // ── Personal scope (always allowed) ────────────────────────────────────────

  it('personal scope: succeeds with no capabilities', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'personal' },
      makeContext({}),
    );
    expect(result.success).toBe(true);
  });

  it('personal scope: succeeds even when agent has no circle', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'personal' },
      makeContext({ circle: '' }),
    );
    expect(result.success).toBe(true);
  });

  // ── Circle scope ────────────────────────────────────────────────────────────

  it('circle scope: succeeds with knowledgeWrite:circle capability', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'circle' },
      makeContext({ capabilities: ['knowledgeWrite:circle'], circle: 'my-circle' }),
    );
    expect(result.success).toBe(true);
  });

  it('circle scope: succeeds with knowledgeWrite:merge-without-approval capability', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'circle' },
      makeContext({ capabilities: ['knowledgeWrite:merge-without-approval'], circle: 'my-circle' }),
    );
    expect(result.success).toBe(true);
  });

  it('circle scope: DENIED without capability', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'circle' },
      makeContext({ capabilities: [], circle: 'my-circle' }),
    );
    expect(result.success).toBe(false);
    expect(result.error).toContain('knowledgeWrite:circle');
  });

  it('circle scope: DENIED when no circle membership', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'circle' },
      makeContext({ capabilities: ['knowledgeWrite:circle'], circle: '' }),
    );
    expect(result.success).toBe(false);
    expect(result.error).toContain('circle');
  });

  // ── Global scope ────────────────────────────────────────────────────────────

  it('global scope: succeeds with knowledgeWrite:global capability', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'global' },
      makeContext({ capabilities: ['knowledgeWrite:global'] }),
    );
    expect(result.success).toBe(true);
  });

  it('global scope: succeeds with knowledgeWrite:merge-without-approval capability', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'global' },
      makeContext({ capabilities: ['knowledgeWrite:merge-without-approval'] }),
    );
    expect(result.success).toBe(true);
  });

  it('global scope: DENIED without capability', async () => {
    const result = await storeSkill.handler(
      { ...baseParams, scope: 'global' },
      makeContext({ capabilities: [] }),
    );
    expect(result.success).toBe(false);
    expect(result.error).toContain('knowledgeWrite:global');
  });
});

describe('knowledge-query scope permission checks', () => {
  const querySkill = createKnowledgeQuerySkill();

  it('personal scope: always accessible', async () => {
    const result = await querySkill.handler(
      { query: 'test', scopes: ['personal'] },
      makeContext({}),
    );
    expect(result.success).toBe(true);
    expect((result.data as any).results).toEqual([]);
  });

  it('global scope: always accessible', async () => {
    const result = await querySkill.handler(
      { query: 'test', scopes: ['global'] },
      makeContext({}),
    );
    expect(result.success).toBe(true);
    expect((result.data as any).results).toEqual([]);
  });

  it('circle scope with no circle membership: returns error marker', async () => {
    const result = await querySkill.handler(
      { query: 'test', scopes: ['circle'] },
      makeContext({ circle: '' }),
    );
    expect(result.success).toBe(true);
    expect((result.data as any).error).toBe('not_a_circle_member');
  });

  it('circle scope with membership: proceeds to search', async () => {
    const result = await querySkill.handler(
      { query: 'test', scopes: ['circle'] },
      makeContext({ circle: 'my-circle' }),
    );
    expect(result.success).toBe(true);
    expect(Array.isArray((result.data as any).results)).toBe(true);
  });

  it('default scopes include personal, circle, and global', async () => {
    const result = await querySkill.handler(
      { query: 'test' },
      makeContext({ circle: 'my-circle' }),
    );
    expect(result.success).toBe(true);
  });

  it('empty results returned as array, not error', async () => {
    const result = await querySkill.handler(
      { query: 'nonexistent topic' },
      makeContext({ circle: 'my-circle' }),
    );
    expect(result.success).toBe(true);
    expect((result.data as any).results).toEqual([]);
  });
});
