import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'fs/promises';
import os from 'os';
import path from 'path';
import { ScopedMemoryBlockStore } from './ScopedMemoryBlockStore.js';
import type { KnowledgeBlockCreateOpts } from './scoped-types.js';

describe('ScopedMemoryBlockStore', () => {
  let tmpDir: string;
  let store: ScopedMemoryBlockStore;
  const agentId = 'test-agent-001';

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-scoped-'));
    store = new ScopedMemoryBlockStore(tmpDir);
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
  });

  const makeOpts = (overrides?: Partial<KnowledgeBlockCreateOpts>): KnowledgeBlockCreateOpts => ({
    content: 'Test knowledge content',
    type: 'fact',
    agentId,
    ...overrides,
  });

  // ── Write / Read ─────────────────────────────────────────────────────────────

  it('writes a block with correct frontmatter fields', async () => {
    const block = await store.write(makeOpts());
    expect(block.id).toBeDefined();
    expect(block.agentId).toBe(agentId);
    expect(block.type).toBe('fact');
    expect(block.timestamp).toMatch(/^\d{4}-/);
    expect(block.tags).toEqual([]);
    expect(block.importance).toBe(3);
  });

  it('writes to {memoryRoot}/{agentId}/{type}/{timestamp}-{id}.md', async () => {
    const block = await store.write(makeOpts({ type: 'insight', tags: ['test'] }));
    const typeDir = path.join(tmpDir, agentId, 'insight');
    const files = await fs.readdir(typeDir);
    expect(files).toHaveLength(1);
    expect(files[0]).toContain(block.id);
    expect(files[0]).toContain('.md');
  });

  it('persists importance and tags in frontmatter', async () => {
    const block = await store.write(
      makeOpts({ importance: 5, tags: ['a', 'b'], title: 'My Title' })
    );
    const typeDir = path.join(tmpDir, agentId, 'fact');
    const files = await fs.readdir(typeDir);
    const raw = await fs.readFile(path.join(typeDir, files[0]!), 'utf8');
    expect(raw).toContain('importance: 5');
    expect(raw).toContain('title: My Title');
    expect(raw).toContain('agentId');
    expect(block.importance).toBe(5);
    expect(block.tags).toEqual(['a', 'b']);
  });

  it('reads a block by id (readByAgent)', async () => {
    const written = await store.write(makeOpts({ content: 'Hello World' }));
    const found = await store.readByAgent(agentId, written.id);
    expect(found).not.toBeNull();
    expect(found!.id).toBe(written.id);
    expect(found!.content).toBe('Hello World');
  });

  it('returns null for nonexistent id', async () => {
    const result = await store.readByAgent(agentId, 'nonexistent-uuid');
    expect(result).toBeNull();
  });

  // ── List ──────────────────────────────────────────────────────────────────────

  it('lists blocks for an agent', async () => {
    await store.write(makeOpts({ type: 'fact' }));
    await store.write(makeOpts({ type: 'insight' }));
    await store.write(makeOpts({ type: 'fact' }));
    const all = await store.list(agentId);
    expect(all).toHaveLength(3);
  });

  it('filters by type', async () => {
    await store.write(makeOpts({ type: 'fact' }));
    await store.write(makeOpts({ type: 'insight' }));
    const facts = await store.list(agentId, { type: 'fact' });
    expect(facts).toHaveLength(1);
    expect(facts[0]!.type).toBe('fact');
  });

  it('filters by tags', async () => {
    await store.write(makeOpts({ tags: ['api', 'v2'] }));
    await store.write(makeOpts({ tags: ['db'] }));
    const results = await store.list(agentId, { tags: ['api'] });
    expect(results).toHaveLength(1);
  });

  it('filters by since date', async () => {
    const block = await store.write(makeOpts());
    const future = new Date(Date.now() + 60_000).toISOString();
    const past = new Date(Date.parse(block.timestamp) - 60_000).toISOString();
    expect(await store.list(agentId, { since: future })).toHaveLength(0);
    expect(await store.list(agentId, { since: past })).toHaveLength(1);
  });

  // ── Delete ────────────────────────────────────────────────────────────────────

  it('deletes a block by id', async () => {
    const block = await store.write(makeOpts());
    const ok = await store.delete(agentId, block.id);
    expect(ok).toBe(true);
    expect(await store.readByAgent(agentId, block.id)).toBeNull();
  });

  it('returns false deleting nonexistent block', async () => {
    expect(await store.delete(agentId, 'no-such-id')).toBe(false);
  });

  // ── Archive ───────────────────────────────────────────────────────────────────

  it('moves block to archive directory', async () => {
    const block = await store.write(makeOpts());
    const newPath = await store.moveToArchive(agentId, block.id);
    expect(newPath).not.toBeNull();
    expect(newPath).toContain('archive');
    // Should no longer be in active list
    expect(await store.list(agentId)).toHaveLength(0);
    // Should be in archive list
    const archived = await store.listArchive(agentId);
    expect(archived).toHaveLength(1);
    expect(archived[0]!.id).toBe(block.id);
  });

  it('returns null moving nonexistent block to archive', async () => {
    expect(await store.moveToArchive(agentId, 'fake-id')).toBeNull();
  });

  // ── countByAgent ─────────────────────────────────────────────────────────────

  it('counts active blocks (excludes archived)', async () => {
    const b1 = await store.write(makeOpts({ type: 'fact' }));
    await store.write(makeOpts({ type: 'memory' }));
    expect(await store.countByAgent(agentId)).toBe(2);
    await store.moveToArchive(agentId, b1.id);
    // archive dir is separate — countByAgent only counts typed dirs
    expect(await store.countByAgent(agentId)).toBe(1);
  });

  // ── Invalid frontmatter ──────────────────────────────────────────────────────

  it('skips malformed files without crashing', async () => {
    const typeDir = path.join(tmpDir, agentId, 'fact');
    await fs.mkdir(typeDir, { recursive: true });
    await fs.writeFile(path.join(typeDir, 'broken.md'), '---\nnot valid: [broken\n---\nContent');
    // Write a valid one
    await store.write(makeOpts());
    const blocks = await store.list(agentId, { type: 'fact' });
    expect(blocks).toHaveLength(1);
  });
});
