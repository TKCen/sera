import fs from 'fs/promises';
import path from 'path';
import matter from 'gray-matter';
import { v4 as uuidv4 } from 'uuid';
import { Mutex } from 'async-mutex';
import type {
  MemoryBlockType,
  MemoryEntry,
  MemoryBlock,
  MemoryGraph,
  GraphNode,
  GraphEdge,
  CreateEntryOptions,
  MemorySource,
} from './types.js';
import { MEMORY_BLOCK_TYPES } from './types.js';

/**
 * File-backed memory store where each entry is a markdown file with YAML frontmatter.
 *
 * Layout: `{basePath}/blocks/{type}/{slugified-title}.md`
 */
export class MemoryBlockStore {
  private readonly basePath: string;
  private readonly fileMutexes = new Map<string, Mutex>();

  constructor(basePath: string) {
    this.basePath = basePath;
  }

  private getMutex(filepath: string): Mutex {
    if (!this.fileMutexes.has(filepath)) {
      this.fileMutexes.set(filepath, new Mutex());
    }
    return this.fileMutexes.get(filepath)!;
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────

  /** Turn a title into a filesystem-safe slug. */
  private slugify(title: string): string {
    return title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '');
  }

  /** Directory for a given block type. */
  private blockDir(type: MemoryBlockType): string {
    return path.join(this.basePath, 'blocks', type);
  }

  /** Ensure the directory for a block type exists. */
  private async ensureBlockDir(type: MemoryBlockType): Promise<void> {
    await fs.mkdir(this.blockDir(type), { recursive: true });
  }

  /** Serialize a MemoryEntry to a markdown string with frontmatter. */
  private serialize(entry: MemoryEntry): string {
    const frontmatter: Record<string, unknown> = {
      id: entry.id,
      title: entry.title,
      type: entry.type,
      tags: entry.tags,
      refs: entry.refs,
      source: entry.source,
      createdAt: entry.createdAt,
      updatedAt: entry.updatedAt,
    };
    return matter.stringify(entry.content, frontmatter);
  }

  /** Parse a markdown file into a MemoryEntry. Returns null if invalid/corrupted. */
  private parse(fileContent: string): MemoryEntry | null {
    try {
      const parsed = matter(fileContent);
      const data = parsed.data as Record<string, unknown>;

      if (!data.id || !data.title || !data.type) {
         console.warn(`[MemoryBlockStore] Skipping entry: Missing required frontmatter fields (id, title, type).`);
         return null;
      }

      return {
        id: data.id as string,
        title: data.title as string,
        type: data.type as MemoryBlockType,
        content: parsed.content.trim(),
        refs: Array.isArray(data.refs) ? data.refs.map(String) : [],
        tags: Array.isArray(data.tags) ? data.tags.map(String) : [],
        source: (data.source as MemorySource | undefined) ?? 'system',
        createdAt: (data.createdAt as string) || new Date().toISOString(),
        updatedAt: (data.updatedAt as string) || new Date().toISOString(),
      };
    } catch (err) {
      console.warn(`[MemoryBlockStore] Skipping entry: Failed to parse frontmatter. Error:`, err);
      return null;
    }
  }

  /** Find the filepath for an entry by scanning all block dirs.  Returns null if not found. */
  private async findEntryFile(id: string): Promise<{ filepath: string; type: MemoryBlockType } | null> {
    for (const type of MEMORY_BLOCK_TYPES) {
      const dir = this.blockDir(type);
      let files: string[];
      try {
        files = await fs.readdir(dir);
      } catch {
        continue;
      }
      for (const file of files) {
        if (!file.endsWith('.md')) continue;
        const filepath = path.join(dir, file);
        const raw = await fs.readFile(filepath, 'utf8');
        try {
          const parsed = matter(raw);
          if ((parsed.data as Record<string, unknown>).id === id) {
            return { filepath, type };
          }
        } catch (err) {
          console.warn(`[MemoryBlockStore] Failed to read frontmatter while searching for ID ${id} in ${filepath}. Error:`, err);
        }
      }
    }
    return null;
  }

  /** Parse wikilinks `[[Title]]` from markdown content. */
  private parseWikilinks(content: string): string[] {
    const matches = content.match(/\[\[([^\]]+)\]\]/g);
    if (!matches) return [];
    return matches.map(m => m.slice(2, -2));
  }

  // ── Block Operations ────────────────────────────────────────────────────────

  /** Load all entries for a given block type. */
  async loadBlock(type: MemoryBlockType): Promise<MemoryBlock> {
    await this.ensureBlockDir(type);
    const dir = this.blockDir(type);
    const files = await fs.readdir(dir);
    const entries: MemoryEntry[] = [];

    for (const file of files) {
      if (!file.endsWith('.md')) continue;
      const filepath = path.join(dir, file);
      const raw = await fs.readFile(filepath, 'utf8');
      const parsedEntry = this.parse(raw);
      if (parsedEntry) {
        entries.push(parsedEntry);
      }
    }

    // Sort by creation date (oldest first)
    entries.sort((a, b) => a.createdAt.localeCompare(b.createdAt));

    return { type, entries };
  }

  /** Load all blocks. */
  async loadAll(): Promise<MemoryBlock[]> {
    const blocks: MemoryBlock[] = [];
    for (const type of MEMORY_BLOCK_TYPES) {
      blocks.push(await this.loadBlock(type));
    }
    return blocks;
  }

  // ── Entry CRUD ──────────────────────────────────────────────────────────────

  /** Create a new memory entry. Returns the created entry. */
  async addEntry(type: MemoryBlockType, opts: CreateEntryOptions): Promise<MemoryEntry> {
    await this.ensureBlockDir(type);
    const now = new Date().toISOString();
    const entry: MemoryEntry = {
      id: uuidv4(),
      title: opts.title,
      type,
      content: opts.content,
      refs: opts.refs ?? [],
      tags: opts.tags ?? [],
      source: opts.source ?? 'system',
      createdAt: now,
      updatedAt: now,
    };
    const filename = `${this.slugify(entry.title)}.md`;
    const filepath = path.join(this.blockDir(type), filename);

    const release = await this.getMutex(filepath).acquire();
    try {
      await fs.writeFile(filepath, this.serialize(entry), 'utf8');
    } finally {
      release();
    }
    return entry;
  }

  /** Retrieve a single entry by UUID. */
  async getEntry(id: string): Promise<MemoryEntry | null> {
    const result = await this.findEntryFile(id);
    if (!result) return null;

    const release = await this.getMutex(result.filepath).acquire();
    try {
      const raw = await fs.readFile(result.filepath, 'utf8');
      return this.parse(raw);
    } catch {
      return null;
    } finally {
      release();
    }
  }

  /** Update an entry's content. */
  async updateEntry(id: string, content: string): Promise<MemoryEntry | null> {
    const result = await this.findEntryFile(id);
    if (!result) return null;

    const release = await this.getMutex(result.filepath).acquire();
    try {
      const raw = await fs.readFile(result.filepath, 'utf8');
      const entry = this.parse(raw);
      if (!entry) return null;

      entry.content = content;
      entry.updatedAt = new Date().toISOString();
      await fs.writeFile(result.filepath, this.serialize(entry), 'utf8');
      return entry;
    } finally {
      release();
    }
  }

  /** Delete an entry by UUID. */
  async deleteEntry(id: string): Promise<boolean> {
    const result = await this.findEntryFile(id);
    if (!result) return false;

    const release = await this.getMutex(result.filepath).acquire();
    try {
      await fs.unlink(result.filepath);
      this.fileMutexes.delete(result.filepath);
      return true;
    } catch {
      return false;
    } finally {
      release();
    }
  }

  /** Move an entry to a different block type (preserves ID and refs). */
  async moveEntry(id: string, toType: MemoryBlockType): Promise<MemoryEntry | null> {
    const result = await this.findEntryFile(id);
    if (!result) return null;

    const releaseOld = await this.getMutex(result.filepath).acquire();
    let entry: MemoryEntry | null = null;
    try {
      const raw = await fs.readFile(result.filepath, 'utf8');
      entry = this.parse(raw);
      if (!entry) return null;

      // Delete from old location
      await fs.unlink(result.filepath);
      this.fileMutexes.delete(result.filepath);
    } finally {
      releaseOld();
    }

    // Write to new location
    entry.type = toType;
    entry.updatedAt = new Date().toISOString();
    await this.ensureBlockDir(toType);
    const filename = `${this.slugify(entry.title)}.md`;
    const filepath = path.join(this.blockDir(toType), filename);

    const releaseNew = await this.getMutex(filepath).acquire();
    try {
      await fs.writeFile(filepath, this.serialize(entry), 'utf8');
    } finally {
      releaseNew();
    }
    return entry;
  }

  // ── Refs (Graph Edges) ──────────────────────────────────────────────────────

  /** Add an explicit ref from one entry to another. */
  async addRef(fromId: string, toId: string): Promise<boolean> {
    const result = await this.findEntryFile(fromId);
    if (!result) return false;

    const release = await this.getMutex(result.filepath).acquire();
    try {
      const raw = await fs.readFile(result.filepath, 'utf8');
      const entry = this.parse(raw);
      if (!entry) return false;

      if (!entry.refs.includes(toId)) {
        entry.refs.push(toId);
        entry.updatedAt = new Date().toISOString();
        await fs.writeFile(result.filepath, this.serialize(entry), 'utf8');
      }
      return true;
    } finally {
      release();
    }
  }

  /** Remove an explicit ref. */
  async removeRef(fromId: string, toId: string): Promise<boolean> {
    const result = await this.findEntryFile(fromId);
    if (!result) return false;

    const release = await this.getMutex(result.filepath).acquire();
    try {
      const raw = await fs.readFile(result.filepath, 'utf8');
      const entry = this.parse(raw);
      if (!entry) return false;

      const idx = entry.refs.indexOf(toId);
      if (idx === -1) return false;
      entry.refs.splice(idx, 1);
      entry.updatedAt = new Date().toISOString();
      await fs.writeFile(result.filepath, this.serialize(entry), 'utf8');
      return true;
    } finally {
      release();
    }
  }

  // ── Graph ───────────────────────────────────────────────────────────────────

  /** Build the full memory graph (nodes + edges) for visualization. */
  async getGraph(): Promise<MemoryGraph> {
    const blocks = await this.loadAll();
    const allEntries = blocks.flatMap(b => b.entries);

    // Build title → id index for wikilink resolution
    const titleToId = new Map<string, string>();
    const idToTitle = new Map<string, string>();
    for (const entry of allEntries) {
      titleToId.set(entry.title.toLowerCase(), entry.id);
      idToTitle.set(entry.id, entry.title);
    }

    const nodes: GraphNode[] = allEntries.map(e => ({
      id: e.id,
      title: e.title,
      type: e.type,
      tags: e.tags,
    }));

    const edges: GraphEdge[] = [];
    const edgeSet = new Set<string>();

    for (const entry of allEntries) {
      // Explicit refs
      for (const ref of entry.refs) {
        if (!idToTitle.has(ref)) {
          console.warn(`[MemoryBlockStore] Broken explicit ref found in entry "${entry.title}" (${entry.id}): refers to non-existent ID ${ref}`);
          continue;
        }

        const key = `${entry.id}->${ref}`;
        if (!edgeSet.has(key)) {
          edgeSet.add(key);
          edges.push({ from: entry.id, to: ref, kind: 'ref' });
        }
      }

      // Implicit wikilinks
      const wikilinks = this.parseWikilinks(entry.content);
      for (const title of wikilinks) {
        const targetId = titleToId.get(title.toLowerCase());
        if (targetId) {
          if (targetId !== entry.id) {
            const key = `${entry.id}->${targetId}`;
            if (!edgeSet.has(key)) {
              edgeSet.add(key);
              edges.push({ from: entry.id, to: targetId, kind: 'wikilink' });
            }
          }
        } else {
          console.warn(`[MemoryBlockStore] Broken wikilink found in entry "${entry.title}" (${entry.id}): [[${title}]] does not exist.`);
        }
      }
    }

    return { nodes, edges };
  }

  /** Scan for orphaned refs and broken wikilinks, and clean up broken explicit refs. */
  async repair(): Promise<void> {
    console.log(`[MemoryBlockStore] Starting repair process...`);
    const blocks = await this.loadAll();
    const allEntries = blocks.flatMap(b => b.entries);

    const validIds = new Set<string>(allEntries.map(e => e.id));
    const titleToId = new Map<string, string>();
    for (const entry of allEntries) {
      titleToId.set(entry.title.toLowerCase(), entry.id);
    }

    let repairedCount = 0;

    for (const entry of allEntries) {
      let changed = false;
      const validRefs: string[] = [];

      for (const ref of entry.refs) {
        if (validIds.has(ref)) {
          validRefs.push(ref);
        } else {
          console.warn(`[MemoryBlockStore] Repairing: Removing broken ref to ${ref} from entry "${entry.title}" (${entry.id})`);
          changed = true;
        }
      }

      // Optional: We can't automatically rewrite markdown content to remove broken [[Wikilinks]] easily without context,
      // but we can log them clearly.
      const wikilinks = this.parseWikilinks(entry.content);
      for (const title of wikilinks) {
        if (!titleToId.has(title.toLowerCase())) {
          console.warn(`[MemoryBlockStore] Repair Warning: Broken wikilink [[${title}]] found in entry "${entry.title}" (${entry.id})`);
        }
      }

      if (changed) {
        entry.refs = validRefs;
        entry.updatedAt = new Date().toISOString();
        const result = await this.findEntryFile(entry.id);
        if (result) {
          const release = await this.getMutex(result.filepath).acquire();
          try {
            await fs.writeFile(result.filepath, this.serialize(entry), 'utf8');
            repairedCount++;
          } finally {
            release();
          }
        }
      }
    }

    console.log(`[MemoryBlockStore] Repair complete. Repaired ${repairedCount} entries.`);
  }

  // ── Search ──────────────────────────────────────────────────────────────────

  /** Full-text search across all entries. */
  async search(query: string, limit?: number): Promise<MemoryEntry[]> {
    const blocks = await this.loadAll();
    const allEntries = blocks.flatMap(b => b.entries);
    const q = query.toLowerCase();

    const results = allEntries.filter(
      e =>
        e.title.toLowerCase().includes(q) ||
        e.content.toLowerCase().includes(q) ||
        e.tags.some(t => t.toLowerCase().includes(q)),
    );

    return limit ? results.slice(0, limit) : results;
  }
}
