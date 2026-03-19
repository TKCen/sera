import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import fs from 'fs/promises';
import os from 'os';
import path from 'path';
import { ScopedMemoryBlockStore } from './blocks/ScopedMemoryBlockStore.js';

// Mock VectorService
vi.mock('../services/vector.service.js', () => ({
  VectorService: class {
    delete = vi.fn().mockResolvedValue(undefined);
    upsert = vi.fn().mockResolvedValue(undefined);
    search = vi.fn().mockResolvedValue([]);
    searchLegacy = vi.fn().mockResolvedValue([]);
    deletePoints = vi.fn().mockResolvedValue(undefined);
    upsertPoints = vi.fn().mockResolvedValue(undefined);
    ensureCollection = vi.fn().mockResolvedValue(undefined);
    getCollectionInfo = vi.fn().mockResolvedValue({ vectorCount: 0 });
    rebuildNamespace = vi.fn().mockResolvedValue(undefined);
  },
  collectionName: (ns: string) => ns,
}));

// Mock pg-boss so tests don't need a real DB
vi.mock('pg-boss', () => ({
  default: class {
    start = vi.fn().mockResolvedValue(undefined);
    stop = vi.fn().mockResolvedValue(undefined);
    schedule = vi.fn().mockResolvedValue(undefined);
    work = vi.fn().mockResolvedValue(undefined);
  },
}));

describe('MemoryCompactionService', () => {
  let tmpDir: string;
  let store: ScopedMemoryBlockStore;
  let MemoryCompactionService: typeof import('./MemoryCompactionService.js').MemoryCompactionService;
  const agentId = 'compact-agent';

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-compact-'));
    store = new ScopedMemoryBlockStore(tmpDir);
    vi.resetModules();
    process.env['MEMORY_PATH'] = tmpDir;
    process.env['MEMORY_ARCHIVE_AFTER_DAYS'] = '30';

    const mod = await import('./MemoryCompactionService.js');
    MemoryCompactionService = mod.MemoryCompactionService;
    (MemoryCompactionService as any).instance = undefined;
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
    delete process.env['MEMORY_PATH'];
    delete process.env['MEMORY_ARCHIVE_AFTER_DAYS'];
  });

  it('archives blocks older than threshold with importance <= 2', async () => {
    // Write blocks with old timestamps by manipulating the files directly
    const oldDate = new Date();
    oldDate.setDate(oldDate.getDate() - 35); // 35 days ago → past threshold

    const block = await store.write({
      content: 'Old low-importance fact',
      type: 'fact',
      agentId,
      importance: 1,
    });

    // Overwrite the file with an old timestamp
    const typeDir = path.join(tmpDir, agentId, 'fact');
    const files = await fs.readdir(typeDir);
    const filePath = path.join(typeDir, files[0]!);
    const raw = await fs.readFile(filePath, 'utf8');
    const updated = raw.replace(block.timestamp, oldDate.toISOString());
    await fs.writeFile(filePath, updated, 'utf8');

    const svc = MemoryCompactionService.getInstance();
    const result = await svc.triggerCompaction(agentId);

    expect(result.blocksArchived).toBe(1);
    expect(result.agentId).toBe(agentId);
    // Block should now be in archive
    const archived = await store.listArchive(agentId);
    expect(archived).toHaveLength(1);
  });

  it('does not archive high-importance blocks', async () => {
    const oldDate = new Date();
    oldDate.setDate(oldDate.getDate() - 35);

    const block = await store.write({
      content: 'Important old fact',
      type: 'fact',
      agentId,
      importance: 4,
    });

    const typeDir = path.join(tmpDir, agentId, 'fact');
    const files = await fs.readdir(typeDir);
    const filePath = path.join(typeDir, files[0]!);
    const raw = await fs.readFile(filePath, 'utf8');
    await fs.writeFile(filePath, raw.replace(block.timestamp, oldDate.toISOString()));

    const svc = MemoryCompactionService.getInstance();
    const result = await svc.triggerCompaction(agentId);

    expect(result.blocksArchived).toBe(0);
    expect(await store.list(agentId)).toHaveLength(1);
  });

  it('does not archive recent low-importance blocks', async () => {
    await store.write({
      content: 'New low-importance fact',
      type: 'fact',
      agentId,
      importance: 1,
    });

    const svc = MemoryCompactionService.getInstance();
    const result = await svc.triggerCompaction(agentId);
    expect(result.blocksArchived).toBe(0);
  });

  it('returns zero counts for agent with no blocks', async () => {
    const svc = MemoryCompactionService.getInstance();
    const result = await svc.triggerCompaction('no-such-agent');
    expect(result.blocksArchived).toBe(0);
    expect(result.vectorsRemoved).toBe(0);
  });
});
