import fs from 'fs/promises';
import fsSync from 'fs';
import path from 'path';
import matter from 'gray-matter';
import { Logger } from '../../lib/logger.js';
import { pool } from '../../lib/database.js';

const logger = new Logger('MemoryBlockStore');
import { v4 as uuidv4 } from 'uuid';
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
  private readonly logicalNamespace: string;
  private cache: Map<MemoryBlockType, MemoryBlock> = new Map();
  private watcher: fsSync.FSWatcher | null = null;

  constructor(basePath: string, logicalNamespace: string = 'global') {
    this.basePath = basePath;
    this.logicalNamespace = logicalNamespace;
    this.initWatcher();
  }

  private initWatcher(): void {
    const blocksDir = path.join(this.basePath, 'blocks');
    if (fsSync.existsSync(blocksDir)) {
      try {
        this.watcher = fsSync.watch(blocksDir, { recursive: true }, (eventType, filename) => {
          if (filename && filename.endsWith('.md')) {
            this.invalidateCache();
          }
        });
        this.watcher.on('error', (err) => {
          logger.error('Watcher error:', err);
          this.destroy();
        });
      } catch (err) {
        logger.error('Failed to initialize watcher:', err);
      }
    }
  }

  /** Invalidates the entire cache, forcing the next loadAll/loadBlock to read from disk. */
  private invalidateCache(): void {
    this.cache.clear();
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
    if (!this.watcher) {
      this.initWatcher(); // Retry watcher initialization if directory was just created
    }
  }

  /** Stop the watcher to prevent memory leaks */
  destroy(): void {
    if (this.watcher) {
      this.watcher.close();
      this.watcher = null;
    }
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
      importance: entry.importance,
    };
    return matter.stringify(entry.content, frontmatter);
  }

  /** Parse a markdown file into a MemoryEntry. */
  private parse(fileContent: string): MemoryEntry {
    const parsed = matter(fileContent);
    const data = parsed.data as Record<string, unknown>;
    const entry: MemoryEntry = {
      id: data.id as string,
      title: data.title as string,
      type: data.type as MemoryBlockType,
      content: parsed.content.trim(),
      refs: (data.refs as string[] | undefined) ?? [],
      tags: (data.tags as string[] | undefined) ?? [],
      source: (data.source as MemorySource | undefined) ?? 'system',
      createdAt: data.createdAt as string,
      updatedAt: data.updatedAt as string,
    };
    if (data.importance !== undefined) {
      entry.importance = data.importance as number;
    }
    return entry;
  }

  /** Find the filepath for an entry by scanning all block dirs.  Returns null if not found. */
  private async findEntryFile(
    id: string
  ): Promise<{ filepath: string; type: MemoryBlockType } | null> {
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
        const parsed = matter(raw);
        if ((parsed.data as Record<string, unknown>).id === id) {
          return { filepath, type };
        }
      }
    }
    return null;
  }

  /** Parse wikilinks `[[Title]]` from markdown content. */
  private parseWikilinks(content: string): string[] {
    const matches = content.match(/\[\[([^\]]+)\]\]/g);
    if (!matches) return [];
    return matches.map((m) => m.slice(2, -2));
  }

  // ── Block Operations ────────────────────────────────────────────────────────

  /** Load all entries for a given block type. */
  async loadBlock(type: MemoryBlockType): Promise<MemoryBlock> {
    if (this.cache.has(type)) {
      return this.cache.get(type)!;
    }

    await this.ensureBlockDir(type);
    const dir = this.blockDir(type);
    const files = await fs.readdir(dir);
    const entries: MemoryEntry[] = [];

    for (const file of files) {
      if (!file.endsWith('.md')) continue;
      try {
        const raw = await fs.readFile(path.join(dir, file), 'utf8');
        const entry = this.parse(raw);
        if (entry.id) {
          entries.push(entry);
        } else {
          logger.warn(`Skipping entry with missing ID in ${file}`);
        }
      } catch (err) {
        logger.error(`Failed to parse ${file}:`, err);
        // Skip malformed entries
      }
    }

    // Sort by creation date (oldest first)
    entries.sort((a, b) => a.createdAt.localeCompare(b.createdAt));

    const block = { type, entries };
    this.cache.set(type, block);
    return block;
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
  async addEntry(
    type: MemoryBlockType,
    opts: CreateEntryOptions,
    agentId?: string
  ): Promise<MemoryEntry> {
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
      importance: opts.importance ?? 3,
    };
    const filename = `${this.slugify(entry.title)}.md`;
    const filepath = path.join(this.blockDir(type), filename);
    await fs.writeFile(filepath, this.serialize(entry), 'utf8');
    this.invalidateCache();

    // Sync to PostgreSQL
    try {
      await pool.query(
        `INSERT INTO memory_blocks (id, agent_id, namespace, type, title, content, tags, importance, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (id) DO UPDATE SET
           agent_id = EXCLUDED.agent_id,
           namespace = EXCLUDED.namespace,
           type = EXCLUDED.type,
           title = EXCLUDED.title,
           content = EXCLUDED.content,
           tags = EXCLUDED.tags,
           importance = EXCLUDED.importance,
           updated_at = EXCLUDED.updated_at`,
        [
          entry.id,
          agentId || null,
          this.logicalNamespace,
          entry.type,
          entry.title,
          entry.content,
          entry.tags,
          entry.importance,
          entry.createdAt,
          entry.updatedAt,
        ]
      );
    } catch (err) {
      logger.error('Failed to sync memory block to PostgreSQL:', err);
    }

    return entry;
  }

  /** Retrieve a single entry by UUID. */
  async getEntry(id: string): Promise<MemoryEntry | null> {
    const entries = await this.getEntries([id]);
    return entries.length > 0 ? entries[0]! : null;
  }

  /** Retrieve multiple entries by UUID. Metadata (type, title) can be provided for faster lookup. */
  async getEntries(
    items: Array<string | { id: string; type?: MemoryBlockType; title?: string }>
  ): Promise<MemoryEntry[]> {
    if (items.length === 0) return [];

    // Normalize items to { id, type, title }
    const normalized = items.map((item) => (typeof item === 'string' ? { id: item } : item));

    const resultsMap = new Map<string, MemoryEntry>();
    const missing = normalized.filter((item) => {
      // 1. Check cache (O(1) per block type)
      for (const block of this.cache.values()) {
        const cached = block.entries.find((e) => e.id === item.id);
        if (cached) {
          resultsMap.set(item.id, cached);
          return false;
        }
      }
      return true;
    });

    if (missing.length > 0) {
      // 2. Fast path: try targeted disk reads if metadata is available
      const stillMissing: typeof missing = [];
      for (const item of missing) {
        if (item.type && item.title) {
          try {
            const filename = `${this.slugify(item.title)}.md`;
            const filepath = path.join(this.blockDir(item.type), filename);
            const raw = await fs.readFile(filepath, 'utf8');
            const entry = this.parse(raw);
            if (entry.id === item.id) {
              resultsMap.set(item.id, entry);
              // We don't populate the full cache here to avoid partial block loads,
              // but we could if we wanted to. For now, just return the entry.
              continue;
            }
          } catch {
            // Fall through to search
          }
        }
        stillMissing.push(item);
      }

      // 3. Slow path: scan all blocks for remaining IDs (O(N) single pass through all files)
      if (stillMissing.length > 0) {
        const missingIds = new Set(stillMissing.map((m) => m.id));
        for (const type of MEMORY_BLOCK_TYPES) {
          if (missingIds.size === 0) break;
          const block = await this.loadBlock(type);
          for (const entry of block.entries) {
            if (missingIds.has(entry.id)) {
              resultsMap.set(entry.id, entry);
              missingIds.delete(entry.id);
            }
          }
        }
      }
    }

    // Preserve input order and handle missing entries (filter nulls)
    return normalized
      .map((item) => resultsMap.get(item.id))
      .filter((e): e is MemoryEntry => e !== undefined);
  }

  /** Update an entry's content. */
  async updateEntry(id: string, content: string): Promise<MemoryEntry | null> {
    const result = await this.findEntryFile(id);
    if (!result) return null;
    const raw = await fs.readFile(result.filepath, 'utf8');
    const entry = this.parse(raw);
    entry.content = content;
    entry.updatedAt = new Date().toISOString();
    await fs.writeFile(result.filepath, this.serialize(entry), 'utf8');
    this.invalidateCache();

    // Sync to PostgreSQL
    try {
      await pool.query(`UPDATE memory_blocks SET content = $1, updated_at = $2 WHERE id = $3`, [
        entry.content,
        entry.updatedAt,
        entry.id,
      ]);
    } catch (err) {
      logger.error('Failed to sync updated memory block to PostgreSQL:', err);
    }

    return entry;
  }

  /** Delete an entry by UUID. */
  async deleteEntry(id: string): Promise<boolean> {
    const result = await this.findEntryFile(id);
    if (!result) return false;
    await fs.unlink(result.filepath);
    this.invalidateCache();

    // Sync to PostgreSQL
    try {
      await pool.query(`DELETE FROM memory_blocks WHERE id = $1`, [id]);
    } catch (err) {
      logger.error('Failed to sync deleted memory block to PostgreSQL:', err);
    }

    return true;
  }

  /** Move an entry to a different block type (preserves ID and refs). */
  async moveEntry(id: string, toType: MemoryBlockType): Promise<MemoryEntry | null> {
    const result = await this.findEntryFile(id);
    if (!result) return null;
    const raw = await fs.readFile(result.filepath, 'utf8');
    const entry = this.parse(raw);

    // Delete from old location
    await fs.unlink(result.filepath);

    // Write to new location
    entry.type = toType;
    entry.updatedAt = new Date().toISOString();
    await this.ensureBlockDir(toType);
    const filename = `${this.slugify(entry.title)}.md`;
    const filepath = path.join(this.blockDir(toType), filename);
    await fs.writeFile(filepath, this.serialize(entry), 'utf8');
    this.invalidateCache();

    // Sync to PostgreSQL
    try {
      await pool.query(`UPDATE memory_blocks SET type = $1, updated_at = $2 WHERE id = $3`, [
        entry.type,
        entry.updatedAt,
        entry.id,
      ]);
    } catch (err) {
      logger.error('Failed to sync moved memory block to PostgreSQL:', err);
    }

    return entry;
  }

  // ── Refs (Graph Edges) ──────────────────────────────────────────────────────

  /** Add an explicit ref from one entry to another. */
  async addRef(fromId: string, toId: string): Promise<boolean> {
    const result = await this.findEntryFile(fromId);
    if (!result) return false;
    const raw = await fs.readFile(result.filepath, 'utf8');
    const entry = this.parse(raw);
    if (!entry.refs.includes(toId)) {
      entry.refs.push(toId);
      entry.updatedAt = new Date().toISOString();
      await fs.writeFile(result.filepath, this.serialize(entry), 'utf8');
      this.invalidateCache();
    }
    return true;
  }

  /** Remove an explicit ref. */
  async removeRef(fromId: string, toId: string): Promise<boolean> {
    const result = await this.findEntryFile(fromId);
    if (!result) return false;
    const raw = await fs.readFile(result.filepath, 'utf8');
    const entry = this.parse(raw);
    const idx = entry.refs.indexOf(toId);
    if (idx === -1) return false;
    entry.refs.splice(idx, 1);
    entry.updatedAt = new Date().toISOString();
    await fs.writeFile(result.filepath, this.serialize(entry), 'utf8');
    this.invalidateCache();
    return true;
  }

  // ── Graph ───────────────────────────────────────────────────────────────────

  /** Build the full memory graph (nodes + edges) for visualization. */
  async getGraph(): Promise<MemoryGraph> {
    const blocks = await this.loadAll();
    const allEntries = blocks.flatMap((b) => b.entries);

    // Build title → id index for wikilink resolution
    const titleToId = new Map<string, string>();
    for (const entry of allEntries) {
      titleToId.set(entry.title.toLowerCase(), entry.id);
    }

    const nodes: GraphNode[] = allEntries.map((e) => ({
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
        if (targetId && targetId !== entry.id) {
          const key = `${entry.id}->${targetId}`;
          if (!edgeSet.has(key)) {
            edgeSet.add(key);
            edges.push({ from: entry.id, to: targetId, kind: 'wikilink' });
          }
        }
      }
    }

    return { nodes, edges };
  }

  // ── Search ──────────────────────────────────────────────────────────────────

  /** Full-text search across all entries. */
  async search(query: string, limit?: number): Promise<MemoryEntry[]> {
    const blocks = await this.loadAll();
    const allEntries = blocks.flatMap((b) => b.entries);
    const q = query.toLowerCase();

    const results = allEntries.filter(
      (e) =>
        e.title.toLowerCase().includes(q) ||
        e.content.toLowerCase().includes(q) ||
        e.tags.some((t) => t.toLowerCase().includes(q))
    );

    return limit ? results.slice(0, limit) : results;
  }

  /** PostgreSQL-backed full-text search. */
  async searchFullText(
    queryText: string,
    namespaces: string[],
    limit: number = 10
  ): Promise<any[]> {
    try {
      const { rows } = await pool.query(
        `SELECT id, agent_id, namespace, type, title, content, tags, importance, created_at, updated_at,
                ts_rank(to_tsvector('english', coalesce(title, '') || ' ' || content), plainto_tsquery('english', $1)) as rank
         FROM memory_blocks
         WHERE namespace = ANY($2)
           AND to_tsvector('english', coalesce(title, '') || ' ' || content) @@ plainto_tsquery('english', $1)
         ORDER BY rank DESC
         LIMIT $3`,
        [queryText, namespaces, limit]
      );

      return rows.map((r) => ({
        id: r.id,
        score: r.rank,
        namespace: r.namespace,
        payload: {
          agent_id: r.agent_id,
          created_at: r.created_at.toISOString(),
          tags: r.tags || [],
          type: r.type,
          title: r.title,
          content: r.content,
          importance: r.importance,
          namespace: r.namespace,
        },
      }));
    } catch (err) {
      logger.error('Full-text search failed:', err);
      return [];
    }
  }
}
