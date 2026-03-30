/**
 * Epic 8 — ScopedMemoryBlockStore
 *
 * File layout: {memoryRoot}/{agentId}/{type}/{timestamp}-{id}.md
 * Each file has YAML frontmatter: id, agentId, type, timestamp, tags, importance, title
 */

import fs from 'fs/promises';
import path from 'path';
import matter from 'gray-matter';
import { v4 as uuidv4 } from 'uuid';
import { Logger } from '../../lib/logger.js';
import type {
  KnowledgeBlock,
  KnowledgeBlockCreateOpts,
  KnowledgeBlockListFilters,
  KnowledgeBlockType,
  Importance,
} from './scoped-types.js';
import { KNOWLEDGE_BLOCK_TYPES } from './scoped-types.js';

const logger = new Logger('ScopedMemoryBlockStore');

export class ScopedMemoryBlockStore {
  constructor(private readonly memoryRoot: string) {}

  private blockDir(agentId: string, type: KnowledgeBlockType): string {
    return path.join(this.memoryRoot, agentId, type);
  }

  private async ensureDir(agentId: string, type: KnowledgeBlockType): Promise<void> {
    await fs.mkdir(this.blockDir(agentId, type), { recursive: true });
  }

  private serialize(block: KnowledgeBlock): string {
    const frontmatter: Record<string, unknown> = {
      id: block.id,
      agentId: block.agentId,
      type: block.type,
      timestamp: block.timestamp,
      tags: block.tags,
      importance: block.importance,
      title: block.title,
      ...(block.compacted !== undefined ? { compacted: block.compacted } : {}),
    };
    return matter.stringify(block.content, frontmatter);
  }

  private parse(raw: string, filePath: string): KnowledgeBlock | null {
    try {
      const parsed = matter(raw);
      const d = parsed.data as Record<string, unknown>;
      if (!d['id'] || !d['agentId'] || !d['type'] || !d['timestamp']) {
        logger.warn(`Invalid frontmatter in ${filePath}, skipping`);
        return null;
      }
      return {
        id: d['id'] as string,
        agentId: d['agentId'] as string,
        type: d['type'] as KnowledgeBlockType,
        timestamp: d['timestamp'] as string,
        tags: Array.isArray(d['tags']) ? (d['tags'] as string[]) : [],
        importance: (typeof d['importance'] === 'number' ? d['importance'] : 3) as Importance,
        title: (d['title'] as string | undefined) ?? '',
        content: parsed.content.trim(),
        ...(d['compacted'] !== undefined ? { compacted: d['compacted'] as boolean } : {}),
      };
    } catch (err) {
      logger.warn(`Failed to parse memory block at ${filePath}:`, err);
      return null;
    }
  }

  private fileName(block: KnowledgeBlock): string {
    // Sanitise ISO timestamp so it is safe in filenames
    const ts = block.timestamp.replace(/[:.]/g, '-');
    return `${ts}-${block.id}.md`;
  }

  async write(opts: KnowledgeBlockCreateOpts): Promise<KnowledgeBlock> {
    const block: KnowledgeBlock = {
      id: uuidv4(),
      agentId: opts.agentId,
      type: opts.type,
      timestamp: new Date().toISOString(),
      tags: opts.tags ?? [],
      importance: opts.importance ?? 3,
      title: opts.title ?? opts.content.slice(0, 80).replace(/\n/g, ' '),
      content: opts.content,
    };
    await this.ensureDir(block.agentId, block.type);
    const filePath = path.join(this.blockDir(block.agentId, block.type), this.fileName(block));
    await fs.writeFile(filePath, this.serialize(block), 'utf8');
    return block;
  }

  async read(id: string): Promise<KnowledgeBlock | null> {
    // Scan all type directories to locate by ID
    for (const type of KNOWLEDGE_BLOCK_TYPES) {
      const dir = path.join(this.memoryRoot);
      // We don't know agentId here — scan all agents
      let agentDirs: string[];
      try {
        agentDirs = await fs.readdir(dir);
      } catch {
        return null;
      }
      for (const agentId of agentDirs) {
        const typeDir = path.join(this.memoryRoot, agentId, type);
        let files: string[];
        try {
          files = await fs.readdir(typeDir);
        } catch {
          continue;
        }
        for (const file of files) {
          if (!file.endsWith('.md')) continue;
          if (!file.includes(id)) continue; // fast path — ID is in filename
          const filePath = path.join(typeDir, file);
          const raw = await fs.readFile(filePath, 'utf8');
          const block = this.parse(raw, filePath);
          if (block?.id === id) return block;
        }
      }
    }
    return null;
  }

  /** Read a block whose file path is known (used by archiver). */
  async readFile(filePath: string): Promise<KnowledgeBlock | null> {
    try {
      const raw = await fs.readFile(filePath, 'utf8');
      return this.parse(raw, filePath);
    } catch {
      return null;
    }
  }

  async readByAgent(agentId: string, id: string): Promise<KnowledgeBlock | null> {
    for (const type of KNOWLEDGE_BLOCK_TYPES) {
      const typeDir = path.join(this.memoryRoot, agentId, type);
      let files: string[];
      try {
        files = await fs.readdir(typeDir);
      } catch {
        continue;
      }
      for (const file of files) {
        if (!file.endsWith('.md') || !file.includes(id)) continue;
        const filePath = path.join(typeDir, file);
        const raw = await fs.readFile(filePath, 'utf8');
        const block = this.parse(raw, filePath);
        if (block?.id === id) return block;
      }
    }
    return null;
  }

  async list(agentId: string, filters?: KnowledgeBlockListFilters): Promise<KnowledgeBlock[]> {
    const results: KnowledgeBlock[] = [];
    const types: readonly KnowledgeBlockType[] = filters?.type
      ? [filters.type]
      : KNOWLEDGE_BLOCK_TYPES;

    for (const type of types) {
      const typeDir = path.join(this.memoryRoot, agentId, type);
      let files: string[];
      try {
        files = await fs.readdir(typeDir);
      } catch {
        continue;
      }
      for (const file of files) {
        if (!file.endsWith('.md')) continue;
        const filePath = path.join(typeDir, file);
        let raw: string;
        try {
          raw = await fs.readFile(filePath, 'utf8');
        } catch {
          continue;
        }
        const block = this.parse(raw, filePath);
        if (!block) continue;

        if (filters?.tags && filters.tags.length > 0) {
          if (!filters.tags.some((t) => block.tags.includes(t))) continue;
        }
        if (filters?.excludeTags && filters.excludeTags.length > 0) {
          if (filters.excludeTags.some((t) => block.tags.includes(t))) continue;
        }
        if (filters?.since && block.timestamp < filters.since) continue;
        if (filters?.before && block.timestamp >= filters.before) continue;
        if (filters?.minImportance !== undefined && block.importance < filters.minImportance)
          continue;

        results.push(block);
      }
    }

    results.sort((a, b) => a.timestamp.localeCompare(b.timestamp));
    return results;
  }

  /** Update an existing block's mutable fields (title, content, tags, importance). */
  async update(
    agentId: string,
    id: string,
    updates: { title?: string; content?: string; tags?: string[]; importance?: Importance }
  ): Promise<KnowledgeBlock | null> {
    const block = await this.readByAgent(agentId, id);
    if (!block) return null;

    const updated: KnowledgeBlock = {
      ...block,
      ...(updates.title !== undefined ? { title: updates.title } : {}),
      ...(updates.content !== undefined ? { content: updates.content } : {}),
      ...(updates.tags !== undefined ? { tags: updates.tags } : {}),
      ...(updates.importance !== undefined ? { importance: updates.importance } : {}),
    };

    // Write back to the same file location
    const filePath = path.join(this.blockDir(agentId, block.type), this.fileName(block));
    await fs.writeFile(filePath, this.serialize(updated), 'utf8');
    return updated;
  }

  async delete(agentId: string, id: string): Promise<boolean> {
    for (const type of KNOWLEDGE_BLOCK_TYPES) {
      const typeDir = path.join(this.memoryRoot, agentId, type);
      let files: string[];
      try {
        files = await fs.readdir(typeDir);
      } catch {
        continue;
      }
      for (const file of files) {
        if (!file.endsWith('.md') || !file.includes(id)) continue;
        const filePath = path.join(typeDir, file);
        const raw = await fs.readFile(filePath, 'utf8').catch(() => null);
        if (!raw) continue;
        const block = this.parse(raw, filePath);
        if (block?.id === id) {
          await fs.unlink(filePath);
          return true;
        }
      }
    }
    return false;
  }

  /**
   * Move a block to the archive directory.
   * Returns the new file path, or null if block not found.
   */
  async moveToArchive(agentId: string, id: string): Promise<string | null> {
    for (const type of KNOWLEDGE_BLOCK_TYPES) {
      const typeDir = path.join(this.memoryRoot, agentId, type);
      let files: string[];
      try {
        files = await fs.readdir(typeDir);
      } catch {
        continue;
      }
      for (const file of files) {
        if (!file.endsWith('.md') || !file.includes(id)) continue;
        const srcPath = path.join(typeDir, file);
        const raw = await fs.readFile(srcPath, 'utf8').catch(() => null);
        if (!raw) continue;
        const block = this.parse(raw, srcPath);
        if (block?.id !== id) continue;

        const archiveDir = path.join(this.memoryRoot, agentId, 'archive');
        await fs.mkdir(archiveDir, { recursive: true });
        const dstPath = path.join(archiveDir, file);
        await fs.rename(srcPath, dstPath);
        return dstPath;
      }
    }
    return null;
  }

  /** List all files in the archive directory for an agent. */
  async listArchive(agentId: string): Promise<KnowledgeBlock[]> {
    const archiveDir = path.join(this.memoryRoot, agentId, 'archive');
    let files: string[];
    try {
      files = await fs.readdir(archiveDir);
    } catch {
      return [];
    }
    const results: KnowledgeBlock[] = [];
    for (const file of files) {
      if (!file.endsWith('.md')) continue;
      const filePath = path.join(archiveDir, file);
      const raw = await fs.readFile(filePath, 'utf8').catch(() => null);
      if (!raw) continue;
      const block = this.parse(raw, filePath);
      if (block) results.push(block);
    }
    return results;
  }

  /** List all agent IDs that have memory directories. */
  async listAgentIds(): Promise<string[]> {
    let entries: string[];
    try {
      entries = await fs.readdir(this.memoryRoot);
    } catch {
      return [];
    }
    // UUID pattern — filters out legacy directories like 'blocks', 'agents', 'circles'
    const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
    const ids: string[] = [];
    for (const entry of entries) {
      if (!uuidPattern.test(entry)) continue;
      try {
        const stat = await fs.stat(path.join(this.memoryRoot, entry));
        if (stat.isDirectory()) ids.push(entry);
      } catch {
        // skip
      }
    }
    return ids;
  }

  /**
   * List blocks across ALL agents, sorted by timestamp descending.
   * Accepts the same filters as `list()` plus an optional `limit`.
   */
  async listAllBlocks(
    filters?: KnowledgeBlockListFilters & { limit?: number }
  ): Promise<KnowledgeBlock[]> {
    const agentIds = await this.listAgentIds();
    const all: KnowledgeBlock[] = [];
    for (const agentId of agentIds) {
      const blocks = await this.list(agentId, filters);
      all.push(...blocks);
    }
    all.sort((a, b) => b.timestamp.localeCompare(a.timestamp));
    if (filters?.limit !== undefined && filters.limit > 0) {
      return all.slice(0, filters.limit);
    }
    return all;
  }

  /** Count blocks by agent across all active (non-archive) types. */
  async countByAgent(agentId: string): Promise<number> {
    let count = 0;
    for (const type of KNOWLEDGE_BLOCK_TYPES) {
      const typeDir = path.join(this.memoryRoot, agentId, type);
      try {
        const files = await fs.readdir(typeDir);
        count += files.filter((f) => f.endsWith('.md')).length;
      } catch {
        // directory absent
      }
    }
    return count;
  }
}
