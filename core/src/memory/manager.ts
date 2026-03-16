import path from 'path';
import { MemoryBlockStore } from './blocks/MemoryBlockStore.js';
import type {
  MemoryBlockType,
  MemoryEntry,
  MemoryBlock,
  MemoryGraph,
  CreateEntryOptions,
} from './blocks/types.js';

/**
 * High-level memory manager backed by the hybrid block store.
 *
 * Provides convenience methods for common memory operations and
 * context assembly for LLM prompts.
 */
export class MemoryManager {
  public readonly store: MemoryBlockStore;
  public readonly circleId?: string;
  public readonly agentId?: string;

  // Simple in-memory rate limiting state
  private static readonly writeTimestamps = new Map<string, number[]>();

  constructor(opts?: { circleId?: string; agentId?: string; basePath?: string }) {
    const rootPath = opts?.basePath
      ?? process.env.MEMORY_PATH
      ?? path.join(process.cwd(), '..', 'memory');

    // Namespace by agent or circle if provided
    let memoryPath = rootPath;
    if (opts?.agentId) {
      memoryPath = path.join(rootPath, 'agents', opts.agentId);
    } else if (opts?.circleId) {
      memoryPath = path.join(rootPath, 'circles', opts.circleId);
    }

    this.store = new MemoryBlockStore(memoryPath);

    if (opts?.circleId !== undefined) {
      this.circleId = opts.circleId;
    }
    if (opts?.agentId !== undefined) {
      this.agentId = opts.agentId;
    }
  }

  // ── Rate Limiting ─────────────────────────────────────────────────────────

  private checkRateLimit(): void {
    const key = this.agentId ?? this.circleId ?? 'global';
    const now = Date.now();
    const timestamps = MemoryManager.writeTimestamps.get(key) ?? [];

    // Filter out timestamps older than 1 minute
    const recent = timestamps.filter(t => now - t < 60000);
    recent.push(now);

    MemoryManager.writeTimestamps.set(key, recent);

    if (recent.length > 10) {
      console.warn(`[MemoryManager] Rate limit warning: More than 10 memory entries written in the last minute by ${key}.`);
    }
  }

  // ── Entry Operations (delegates to store) ─────────────────────────────────

  async addEntry(type: MemoryBlockType, opts: CreateEntryOptions): Promise<MemoryEntry> {
    if (!type || typeof type !== 'string') throw new Error('Invalid type parameter');
    if (!opts || typeof opts !== 'object') throw new Error('Invalid options parameter');
    if (!opts.title || typeof opts.title !== 'string') throw new Error('opts.title is required');
    if (typeof opts.content !== 'string') throw new Error('opts.content is required');

    this.checkRateLimit();
    return this.store.addEntry(type, opts);
  }

  async getEntry(id: string): Promise<MemoryEntry | null> {
    if (!id || typeof id !== 'string') throw new Error('Invalid id parameter');
    return this.store.getEntry(id);
  }

  async updateEntry(id: string, content: string): Promise<MemoryEntry | null> {
    if (!id || typeof id !== 'string') throw new Error('Invalid id parameter');
    if (typeof content !== 'string') throw new Error('Invalid content parameter');

    this.checkRateLimit();
    return this.store.updateEntry(id, content);
  }

  async deleteEntry(id: string): Promise<boolean> {
    if (!id || typeof id !== 'string') throw new Error('Invalid id parameter');
    return this.store.deleteEntry(id);
  }

  // ── Block Operations ────────────────────────────────────────────────────────

  async getBlock(type: MemoryBlockType): Promise<MemoryBlock> {
    if (!type || typeof type !== 'string') throw new Error('Invalid type parameter');
    return this.store.loadBlock(type);
  }

  async getAllBlocks(): Promise<MemoryBlock[]> {
    return this.store.loadAll();
  }

  // ── Ref Operations ──────────────────────────────────────────────────────────

  async addRef(fromId: string, toId: string): Promise<boolean> {
    if (!fromId || typeof fromId !== 'string') throw new Error('Invalid fromId parameter');
    if (!toId || typeof toId !== 'string') throw new Error('Invalid toId parameter');
    this.checkRateLimit();
    return this.store.addRef(fromId, toId);
  }

  async removeRef(fromId: string, toId: string): Promise<boolean> {
    if (!fromId || typeof fromId !== 'string') throw new Error('Invalid fromId parameter');
    if (!toId || typeof toId !== 'string') throw new Error('Invalid toId parameter');
    this.checkRateLimit();
    return this.store.removeRef(fromId, toId);
  }

  // ── Graph ───────────────────────────────────────────────────────────────────

  async getGraph(): Promise<MemoryGraph> {
    return this.store.getGraph();
  }

  // ── Search ──────────────────────────────────────────────────────────────────

  async search(query: string, limit?: number): Promise<MemoryEntry[]> {
    if (typeof query !== 'string') return [];
    if (!query.trim()) return [];
    if (limit !== undefined && (typeof limit !== 'number' || limit <= 0)) {
       throw new Error('Limit must be a positive number');
    }
    const results = await this.store.search(query, limit);
    return results || [];
  }

  // ── Context Assembly ────────────────────────────────────────────────────────

  /**
   * Assemble a working-memory context string from human, persona, and core blocks.
   * This is intended for injection into LLM system/user prompts.
   */
  async assembleContext(): Promise<string> {
    const sections: string[] = [];

    const human = await this.store.loadBlock('human');
    if (human.entries.length > 0) {
      sections.push('## User Context');
      for (const e of human.entries) {
        sections.push(`### ${e.title}\n${e.content}`);
      }
    }

    const persona = await this.store.loadBlock('persona');
    if (persona.entries.length > 0) {
      sections.push('## Self-Model');
      for (const e of persona.entries) {
        sections.push(`### ${e.title}\n${e.content}`);
      }
    }

    const core = await this.store.loadBlock('core');
    if (core.entries.length > 0) {
      sections.push('## Core Knowledge');
      for (const e of core.entries) {
        sections.push(`### ${e.title}\n${e.content}`);
      }
    }

    return sections.join('\n\n');
  }

  /**
   * Move an entry from its current block to the archive block.
   * Preserves ID, refs, and all metadata.
   */
  async archiveEntry(entryId: string): Promise<MemoryEntry | null> {
    if (!entryId || typeof entryId !== 'string') throw new Error('Invalid entryId parameter');
    this.checkRateLimit();
    return this.store.moveEntry(entryId, 'archive');
  }
}
