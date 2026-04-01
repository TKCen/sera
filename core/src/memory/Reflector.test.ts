import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import fs from 'fs/promises';
import path from 'path';
import os from 'os';
import { Reflector } from './Reflector.js';
import { MemoryManager } from './manager.js';
import type { LLMProvider } from '../lib/llm/types.js';

// Mock external services so tests don't need infrastructure
vi.mock('../services/vector.service.js', () => ({
  VectorService: class {
    ensureCollection = vi.fn().mockResolvedValue(undefined);
    upsertPoints = vi.fn().mockResolvedValue(undefined);
    deletePoints = vi.fn().mockResolvedValue(undefined);
    searchLegacy = vi.fn().mockResolvedValue([]);
  },
}));

vi.mock('../services/embedding.service.js', () => ({
  EmbeddingService: {
    getInstance: () => ({
      isAvailable: () => false,
      generateEmbedding: vi.fn().mockResolvedValue([]),
    }),
  },
}));

vi.mock('../audit/index.js', () => ({
  AuditService: {
    getInstance: () => ({
      record: vi.fn().mockResolvedValue(undefined),
    }),
  },
}));

/** Create a mock LLM provider that returns a canned summary. */
function createMockLLM(summary: string = 'Compacted summary of entries.'): LLMProvider {
  return {
    chat: vi.fn().mockResolvedValue({ content: summary }),
    async *chatStream() {
      yield { token: summary, done: true };
    },
  };
}

describe('Reflector', () => {
  let tmpDir: string;
  let manager: MemoryManager;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-reflector-test-'));
    manager = new MemoryManager({ basePath: tmpDir });
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
  });

  it('should not compact when under threshold', async () => {
    await manager.addEntry('core', { title: 'Single Entry', content: 'Not enough to trigger.' });

    const llm = createMockLLM();
    const result = await Reflector.compactIfNeeded(manager, llm, { threshold: 5 });

    expect(result).toBeNull();
    expect(llm.chat).not.toHaveBeenCalled();
  });

  it('should compact oldest entries when over threshold', async () => {
    // Create 7 entries — threshold 5, batch 3 → should compact the 3 oldest
    for (let i = 0; i < 7; i++) {
      await manager.addEntry('core', {
        title: `Entry ${i}`,
        content: `Content for entry ${i}.`,
        source: 'agent',
      });
    }

    const llm = createMockLLM('Summary of entries 0, 1, 2.');
    const result = await Reflector.compactIfNeeded(manager, llm, { threshold: 5, batchSize: 3 });

    expect(result).not.toBeNull();
    expect(result!.type).toBe('archive');
    expect(result!.source).toBe('reflector');
    expect(result!.tags).toContain('compaction');
    expect(result!.content).toBe('Summary of entries 0, 1, 2.');

    // Summary should ref back to the 3 compacted entries
    expect(result!.refs).toHaveLength(3);

    // The LLM should have been called once
    expect(llm.chat).toHaveBeenCalledOnce();

    // Core block should now have 4 entries (7 - 3 moved)
    const coreBlock = await manager.getBlock('core');
    expect(coreBlock.entries).toHaveLength(4);

    // Archive should have 4 entries (3 moved originals + 1 summary)
    const archiveBlock = await manager.getBlock('archive');
    expect(archiveBlock.entries).toHaveLength(4);
  });

  it('should preserve ref chain for traceability', async () => {
    // Create entries with refs
    const e1 = await manager.addEntry('core', { title: 'E1', content: 'First' });
    const e2 = await manager.addEntry('core', { title: 'E2', content: 'Second', refs: [e1.id] });
    await manager.addEntry('core', { title: 'E3', content: 'Third' });
    // Fill up to exceed threshold
    for (let i = 0; i < 3; i++) {
      await manager.addEntry('core', { title: `Filler ${i}`, content: 'filler' });
    }

    const llm = createMockLLM('Summary.');
    const result = await Reflector.compactIfNeeded(manager, llm, { threshold: 3, batchSize: 3 });

    expect(result).not.toBeNull();

    // The moved entry E2 should still have its ref to E1
    const movedE2 = await manager.getEntry(e2.id);
    expect(movedE2).not.toBeNull();
    expect(movedE2!.type).toBe('archive');
    expect(movedE2!.refs).toContain(e1.id);
  });
});
