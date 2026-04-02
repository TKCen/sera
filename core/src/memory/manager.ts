import path from 'path';
import { MemoryBlockStore } from './blocks/MemoryBlockStore.js';
import { VectorService } from '../services/vector.service.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { Logger } from '../lib/logger.js';
import { AuditService } from '../audit/index.js';

const logger = new Logger('MemoryManager');
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
  private readonly vectorService: VectorService;
  private readonly embeddingService = EmbeddingService.getInstance();

  // Simple in-memory rate limiting state
  private static readonly writeTimestamps = new Map<string, number[]>();

  constructor(opts?: { circleId?: string; agentId?: string; basePath?: string }) {
    const rootPath =
      opts?.basePath ?? process.env.MEMORY_PATH ?? path.join(process.cwd(), '..', 'memory');

    // Namespace by agent or circle if provided
    let memoryPath = rootPath;
    if (opts?.agentId) {
      memoryPath = path.join(rootPath, 'agents', opts.agentId);
    } else if (opts?.circleId) {
      memoryPath = path.join(rootPath, 'circles', opts.circleId);
    }

    this.store = new MemoryBlockStore(memoryPath);
    this.vectorService = new VectorService(`memory_${opts?.circleId || opts?.agentId || 'global'}`);

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
    const recent = timestamps.filter((t) => now - t < 60000);
    recent.push(now);

    MemoryManager.writeTimestamps.set(key, recent);

    if (recent.length > 10) {
      logger.warn(`Rate limit: More than 10 memory entries written in the last minute by ${key}.`);
    }
  }

  // ── Entry Operations (delegates to store) ─────────────────────────────────

  async addEntry(type: MemoryBlockType, opts: CreateEntryOptions): Promise<MemoryEntry> {
    if (!type || typeof type !== 'string') throw new Error('Invalid type parameter');
    if (!opts || typeof opts !== 'object') throw new Error('Invalid options parameter');
    if (!opts.title || typeof opts.title !== 'string') throw new Error('opts.title is required');
    if (typeof opts.content !== 'string') throw new Error('opts.content is required');

    this.checkRateLimit();
    const entry = await this.store.addEntry(type, opts);
    await this.indexEntry(entry);

    const auditId = this.agentId || this.circleId;
    if (auditId) {
      try {
        await AuditService.getInstance().record({
          actorType: this.agentId ? 'agent' : 'system',
          actorId: auditId,
          actingContext: null,
          eventType: 'memory.add',
          payload: {
            type,
            id: entry.id,
            title: entry.title,
            source: entry.source,
          },
        });
      } catch (auditErr) {
        logger.error('Failed to record audit entry:', auditErr);
      }
    }
    return entry;
  }

  async getEntry(id: string): Promise<MemoryEntry | null> {
    if (!id || typeof id !== 'string') throw new Error('Invalid id parameter');
    return this.store.getEntry(id);
  }

  async updateEntry(id: string, content: string): Promise<MemoryEntry | null> {
    if (!id || typeof id !== 'string') throw new Error('Invalid id parameter');
    if (typeof content !== 'string') throw new Error('Invalid content parameter');

    this.checkRateLimit();
    const entry = await this.store.updateEntry(id, content);
    if (entry) {
      await this.indexEntry(entry);
    }

    const auditId = this.agentId || this.circleId;
    if (auditId && entry) {
      try {
        await AuditService.getInstance().record({
          actorType: this.agentId ? 'agent' : 'system',
          actorId: auditId,
          actingContext: null,
          eventType: 'memory.update',
          payload: {
            id: entry.id,
            title: entry.title,
            type: entry.type,
          },
        });
      } catch (auditErr) {
        logger.error('Failed to record audit entry:', auditErr);
      }
    }
    return entry;
  }

  async deleteEntry(id: string): Promise<boolean> {
    if (!id || typeof id !== 'string') throw new Error('Invalid id parameter');
    const entry = await this.getEntry(id);
    const deleted = await this.store.deleteEntry(id);

    if (deleted) {
      await this.vectorService.deletePoints([id]);
    }

    const auditId = this.agentId || this.circleId;
    if (auditId && deleted && entry) {
      try {
        await AuditService.getInstance().record({
          actorType: this.agentId ? 'agent' : 'system',
          actorId: auditId,
          actingContext: null,
          eventType: 'memory.delete',
          payload: {
            id: entry.id,
            title: entry.title,
            type: entry.type,
          },
        });
      } catch (auditErr) {
        logger.error('Failed to record audit entry:', auditErr);
      }
    }

    return deleted;
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

  async search(query: string, limit: number = 5): Promise<MemoryEntry[]> {
    if (typeof query !== 'string') return [];
    if (!query.trim()) return [];
    if (limit !== undefined && (typeof limit !== 'number' || limit <= 0)) {
      throw new Error('Limit must be a positive number');
    }

    try {
      const vector = await this.embeddingService.generateEmbedding(query);
      const vectorResults = await this.vectorService.searchLegacy(vector, limit);

      const entries: MemoryEntry[] = [];
      for (const res of vectorResults) {
        const entry = await this.getEntry(res.id as string);
        if (entry) entries.push(entry);
      }

      if (entries.length > 0) return entries;
    } catch (err) {
      logger.error('Vector search failed, falling back to text search:', err);
    }

    const results = await this.store.search(query, limit);
    return results || [];
  }

  // ── Context Assembly ────────────────────────────────────────────────────────

  /**
   * Assemble a working-memory context string from human, persona, and core blocks.
   * This is intended for injection into LLM system/user prompts.
   * If a query is provided, it also performs a vector search for relevant archival memory.
   */
  async assembleContext(query?: string): Promise<string> {
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

    if (query) {
      try {
        const vector = await this.embeddingService.generateEmbedding(query);
        // Search specifically in the 'archive' type if possible, or just general search
        const vectorResults = await this.vectorService.searchLegacy(vector, 3, {
          must: [{ key: 'type', match: { value: 'archive' } }],
        });

        if (vectorResults.length > 0) {
          sections.push('## Relevant Archival Memory');
          for (const res of vectorResults) {
            const p = res.payload as Record<string, unknown> | undefined;
            sections.push(`### ${p?.['title'] || 'Unknown'}\n${p?.['content'] || ''}`);
          }
        }
      } catch (err) {
        logger.error('Failed to fetch archival context:', err);
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
    const entry = await this.store.moveEntry(entryId, 'archive');
    if (entry) {
      await this.indexEntry(entry);
    }
    return entry;
  }

  private async indexEntry(entry: MemoryEntry): Promise<void> {
    try {
      // nomic-embed-text produces 768-dim vectors
      await this.vectorService.ensureCollection(768);
      const embedding = await this.embeddingService.generateEmbedding(
        `${entry.title}\n${entry.content}`
      );
      await this.vectorService.upsertPoints([
        {
          id: entry.id,
          vector: embedding,
          payload: {
            title: entry.title,
            content: entry.content,
            type: entry.type,
            tags: entry.tags,
          },
        },
      ]);
    } catch (err) {
      logger.error(`Failed to index entry ${entry.id}:`, err);
    }
  }
}
