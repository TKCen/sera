import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'fs/promises';
import path from 'path';
import os from 'os';
import { MemoryBlockStore } from './MemoryBlockStore.js';
import type { MemoryBlockType } from './types.js';

describe('MemoryBlockStore', () => {
  let tmpDir: string;
  let store: MemoryBlockStore;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-memory-test-'));
    store = new MemoryBlockStore(tmpDir);
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
  });

  // ── CRUD ────────────────────────────────────────────────────────────────────

  it('should create and retrieve an entry', async () => {
    const entry = await store.addEntry('core', {
      title: 'Test Entry',
      content: 'This is test content.',
      tags: ['test'],
      source: 'user',
    });

    expect(entry.id).toBeDefined();
    expect(entry.title).toBe('Test Entry');
    expect(entry.type).toBe('core');
    expect(entry.content).toBe('This is test content.');
    expect(entry.tags).toEqual(['test']);
    expect(entry.source).toBe('user');

    const retrieved = await store.getEntry(entry.id);
    expect(retrieved).not.toBeNull();
    expect(retrieved!.id).toBe(entry.id);
    expect(retrieved!.content).toBe('This is test content.');
  });

  it('should persist entries as markdown files with frontmatter', async () => {
    await store.addEntry('human', {
      title: 'User Preferences',
      content: 'Prefers dark mode and concise responses.',
    });

    const dir = path.join(tmpDir, 'blocks', 'human');
    const files = await fs.readdir(dir);
    expect(files).toContain('user-preferences.md');

    const raw = await fs.readFile(path.join(dir, 'user-preferences.md'), 'utf8');
    expect(raw).toContain('title: User Preferences');
    expect(raw).toContain('type: human');
    expect(raw).toContain('Prefers dark mode');
  });

  it('should update entry content', async () => {
    const entry = await store.addEntry('core', {
      title: 'Mutable Entry',
      content: 'Version 1',
    });

    const updated = await store.updateEntry(entry.id, 'Version 2');
    expect(updated).not.toBeNull();
    expect(updated!.content).toBe('Version 2');

    const retrieved = await store.getEntry(entry.id);
    expect(retrieved!.content).toBe('Version 2');
  });

  it('should delete an entry', async () => {
    const entry = await store.addEntry('core', {
      title: 'Deletable',
      content: 'Will be deleted.',
    });

    const ok = await store.deleteEntry(entry.id);
    expect(ok).toBe(true);

    const retrieved = await store.getEntry(entry.id);
    expect(retrieved).toBeNull();
  });

  it('returns null for nonexistent entry', async () => {
    const result = await store.getEntry('nonexistent-id');
    expect(result).toBeNull();
  });

  // ── Blocks ──────────────────────────────────────────────────────────────────

  it('should load a block with all its entries', async () => {
    await store.addEntry('core', { title: 'Entry A', content: 'Content A' });
    await store.addEntry('core', { title: 'Entry B', content: 'Content B' });
    await store.addEntry('human', { title: 'Entry C', content: 'Content C' });

    const coreBlock = await store.loadBlock('core');
    expect(coreBlock.type).toBe('core');
    expect(coreBlock.entries).toHaveLength(2);

    const humanBlock = await store.loadBlock('human');
    expect(humanBlock.entries).toHaveLength(1);
  });

  it('should load all blocks (empty ones too)', async () => {
    const blocks = await store.loadAll();
    expect(blocks).toHaveLength(4);
    const types = blocks.map(b => b.type);
    expect(types).toContain('human');
    expect(types).toContain('persona');
    expect(types).toContain('core');
    expect(types).toContain('archive');
  });

  // ── Move ────────────────────────────────────────────────────────────────────

  it('should move an entry between block types', async () => {
    const entry = await store.addEntry('core', {
      title: 'Movable',
      content: 'Will move to archive.',
      refs: [],
    });

    const moved = await store.moveEntry(entry.id, 'archive');
    expect(moved).not.toBeNull();
    expect(moved!.type).toBe('archive');

    // Should not exist in core anymore
    const coreBlock = await store.loadBlock('core');
    expect(coreBlock.entries).toHaveLength(0);

    // Should exist in archive
    const archiveBlock = await store.loadBlock('archive');
    expect(archiveBlock.entries).toHaveLength(1);
    expect(archiveBlock.entries[0]!.id).toBe(entry.id);
  });

  // ── Refs ────────────────────────────────────────────────────────────────────

  it('should add and remove refs', async () => {
    const a = await store.addEntry('core', { title: 'A', content: 'Entry A' });
    const b = await store.addEntry('core', { title: 'B', content: 'Entry B' });

    const ok = await store.addRef(a.id, b.id);
    expect(ok).toBe(true);

    let retrieved = await store.getEntry(a.id);
    expect(retrieved!.refs).toContain(b.id);

    // Adding the same ref again should be idempotent
    await store.addRef(a.id, b.id);
    retrieved = await store.getEntry(a.id);
    expect(retrieved!.refs.filter(r => r === b.id)).toHaveLength(1);

    const removed = await store.removeRef(a.id, b.id);
    expect(removed).toBe(true);

    retrieved = await store.getEntry(a.id);
    expect(retrieved!.refs).not.toContain(b.id);
  });

  // ── Graph ───────────────────────────────────────────────────────────────────

  it('should build a graph with explicit refs', async () => {
    const a = await store.addEntry('core', { title: 'A', content: 'Entry A' });
    const b = await store.addEntry('core', { title: 'B', content: 'Entry B' });
    await store.addRef(a.id, b.id);

    const graph = await store.getGraph();
    expect(graph.nodes).toHaveLength(2);
    expect(graph.edges).toHaveLength(1);
    expect(graph.edges[0]!.from).toBe(a.id);
    expect(graph.edges[0]!.to).toBe(b.id);
    expect(graph.edges[0]!.kind).toBe('ref');
  });

  it('should build a graph with wikilinks', async () => {
    const a = await store.addEntry('core', {
      title: 'Main Topic',
      content: 'This references [[Sub Topic]] for details.',
    });
    const b = await store.addEntry('core', {
      title: 'Sub Topic',
      content: 'Supporting info.',
    });

    const graph = await store.getGraph();
    expect(graph.nodes).toHaveLength(2);
    expect(graph.edges).toHaveLength(1);
    expect(graph.edges[0]!.from).toBe(a.id);
    expect(graph.edges[0]!.to).toBe(b.id);
    expect(graph.edges[0]!.kind).toBe('wikilink');
  });

  // ── Search ──────────────────────────────────────────────────────────────────

  it('should search by content', async () => {
    await store.addEntry('core', { title: 'Vitest Info', content: 'Uses Vitest for testing.' });
    await store.addEntry('core', { title: 'Docker Info', content: 'Runs in Docker containers.' });

    const results = await store.search('vitest');
    expect(results).toHaveLength(1);
    expect(results[0]!.title).toBe('Vitest Info');
  });

  it('should search by tag', async () => {
    await store.addEntry('core', { title: 'Tagged', content: 'Content', tags: ['special'] });
    await store.addEntry('core', { title: 'Untagged', content: 'Content' });

    const results = await store.search('special');
    expect(results).toHaveLength(1);
    expect(results[0]!.title).toBe('Tagged');
  });

  it('should respect search limit', async () => {
    for (let i = 0; i < 10; i++) {
      await store.addEntry('core', { title: `Entry ${i}`, content: 'Matching content' });
    }

    const results = await store.search('matching', 3);
    expect(results).toHaveLength(3);
  });

  // ── Edge Cases ──────────────────────────────────────────────────────────────

  it('should skip malformed YAML frontmatter without crashing', async () => {
    const dir = path.join(tmpDir, 'blocks', 'core');
    await fs.mkdir(dir, { recursive: true });

    // Write a deliberately broken file
    await fs.writeFile(
      path.join(dir, 'broken.md'),
      '---\nmissing id and title and type\n---\nSome content'
    );

    // Write a valid file
    await store.addEntry('core', { title: 'Valid Entry', content: 'Valid content' });

    // Should load the valid one and ignore the broken one
    const block = await store.loadBlock('core');
    expect(block.entries).toHaveLength(1);
    expect(block.entries[0]!.title).toBe('Valid Entry');
  });

  it('should handle concurrent writes to the same block type', async () => {
    const promises: Promise<any>[] = [];
    for (let i = 0; i < 20; i++) {
      promises.push(
        store.addEntry('core', { title: `Concurrent Entry ${i}`, content: `Content ${i}` })
      );
    }

    await Promise.all(promises);

    const block = await store.loadBlock('core');
    expect(block.entries).toHaveLength(20);
  });

  it('should handle concurrent updates to the same entry gracefully', async () => {
    const entry = await store.addEntry('core', { title: 'Shared Entry', content: 'Initial' });

    const promises: Promise<any>[] = [];
    for (let i = 0; i < 10; i++) {
      promises.push(store.updateEntry(entry.id, `Update ${i}`));
    }

    await Promise.all(promises);

    const retrieved = await store.getEntry(entry.id);
    expect(retrieved).not.toBeNull();
    expect(retrieved!.content).toMatch(/Update \d/);
  });

  it('should handle very large content', async () => {
    // 150KB of content
    const largeContent = 'A'.repeat(150 * 1024);

    const entry = await store.addEntry('core', {
      title: 'Large Entry',
      content: largeContent
    });

    const retrieved = await store.getEntry(entry.id);
    expect(retrieved).not.toBeNull();
    expect(retrieved!.content).toBe(largeContent);
  });

  it('should handle special characters in titles', async () => {
    const weirdTitle = 'My @Weird #Title! (With spaces)';
    const entry = await store.addEntry('core', { title: weirdTitle, content: 'Special characters.' });

    expect(entry.title).toBe(weirdTitle);

    const retrieved = await store.getEntry(entry.id);
    expect(retrieved).not.toBeNull();
    expect(retrieved!.title).toBe(weirdTitle);

    // Check slugified filename
    const dir = path.join(tmpDir, 'blocks', 'core');
    const files = await fs.readdir(dir);
    expect(files).toContain('my-weird-title-with-spaces.md');
  });
});
