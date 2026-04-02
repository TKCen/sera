import { QdrantClient } from '@qdrant/js-client-rest';
import { Logger } from '../lib/logger.js';
import { EmbeddingService, getEmbeddingDimension } from './embedding.service.js';
import type { ScopedMemoryBlockStore } from '../memory/blocks/ScopedMemoryBlockStore.js';

const logger = new Logger('VectorService');

// ── Namespace helpers ──────────────────────────────────────────────────────────

export type MemoryNamespace = `personal:${string}` | `circle:${string}` | 'global';

export function collectionName(namespace: MemoryNamespace): string {
  if (namespace === 'global') return 'memory_global';
  if (namespace.startsWith('personal:')) return `memory_personal_${namespace.slice(9)}`;
  if (namespace.startsWith('circle:')) return `memory_circle_${namespace.slice(7)}`;
  throw new Error(`Unknown namespace format: ${namespace}`);
}

// ── Payload metadata ──────────────────────────────────────────────────────────

export interface VectorPayload {
  source_file?: string;
  commit_hash?: string;
  agent_id: string;
  created_at: string;
  tags: string[];
  type: string;
  title?: string;
  content?: string;
  namespace: MemoryNamespace;
  [key: string]: unknown;
}

export interface VectorPoint {
  id: string | number;
  vector: number[];
  payload: VectorPayload;
}

export interface SearchResult {
  id: string | number;
  score: number;
  payload: VectorPayload;
  namespace: MemoryNamespace;
  vector?: number[];
}

export interface HybridSearchConfig {
  vectorWeight: number;
  textWeight: number;
  minScore: number;
  maxResults: number;
  mmr?: {
    enabled: boolean;
    lambda: number;
    candidateMultiplier: number;
  };
  temporalDecay?: {
    enabled: boolean;
    halfLifeDays: number;
  };
}

export interface SearchFilter {
  type?: string;
  tags?: string[];
  excludeTags?: string[];
  since?: string;
  author?: string;
  minImportance?: number;
}

// ── VectorService ─────────────────────────────────────────────────────────────

export class VectorService {
  private client: QdrantClient;
  /** @deprecated Use the scoped methods. Kept for MemoryManager backward compat. */
  private legacyCollectionName: string;

  constructor(legacyCollectionName = 'codebase') {
    this.legacyCollectionName = legacyCollectionName;
    this.client = new QdrantClient({
      url: process.env.QDRANT_URL ?? 'http://localhost:6333',
    });
  }

  // ── Collection management ──────────────────────────────────────────────────

  /** Ensure a Qdrant collection exists for the given namespace. */
  async ensureNamespaceCollection(namespace: MemoryNamespace): Promise<void> {
    const name = collectionName(namespace);
    await this.ensureCollectionByName(name);
  }

  private async ensureCollectionByName(
    name: string,
    vectorSize = getEmbeddingDimension(),
    attempt = 0
  ): Promise<void> {
    const maxAttempts = 5;
    try {
      const collections = await this.client.getCollections();
      if (collections.collections.some((c) => c.name === name)) return;
      await this.client.createCollection(name, {
        vectors: { size: vectorSize, distance: 'Cosine' },
      });
      logger.info(`Qdrant collection "${name}" created`);
    } catch (err) {
      if (attempt >= maxAttempts - 1) {
        logger.error(`Qdrant: failed to ensure collection "${name}" after ${maxAttempts} attempts`);
        throw err;
      }
      const delay = Math.min(1000 * 2 ** attempt, 16_000);
      logger.warn(`Qdrant connection attempt ${attempt + 1} failed, retrying in ${delay}ms`);
      await new Promise((r) => setTimeout(r, delay));
      return this.ensureCollectionByName(name, vectorSize, attempt + 1);
    }
  }

  // ── Upsert ─────────────────────────────────────────────────────────────────

  async upsert(
    blockId: string,
    namespace: MemoryNamespace,
    vector: number[],
    payload: VectorPayload
  ): Promise<void> {
    const name = collectionName(namespace);
    await this.ensureCollectionByName(name);
    await this.client.upsert(name, {
      wait: true,
      points: [{ id: blockId, vector, payload }],
    });
  }

  // ── Search ─────────────────────────────────────────────────────────────────

  /**
   * Search across multiple namespaces in one logical call.
   * Results are tagged with their source namespace and merged by score.
   */
  async search(
    namespaces: MemoryNamespace[],
    queryVector: number[],
    topK: number,
    filter?: SearchFilter,
    withVectors = false
  ): Promise<SearchResult[]> {
    const perNamespace = Math.max(Math.ceil(topK / namespaces.length), topK);
    const allResults: SearchResult[] = [];

    await Promise.all(
      namespaces.map(async (ns) => {
        const name = collectionName(ns);
        try {
          const qdrantFilter = buildQdrantFilter(filter);
          const searchParams: Parameters<typeof this.client.search>[1] = {
            vector: queryVector,
            limit: perNamespace,
            with_payload: true,
            with_vector: withVectors,
          };
          if (qdrantFilter) {
            searchParams.filter = qdrantFilter as Record<string, unknown>;
          }
          const results = await this.client.search(name, searchParams);
          for (const r of results) {
            allResults.push({
              id: r.id,
              score: r.score,
              payload: (r.payload ?? {}) as VectorPayload,
              namespace: ns,
              vector: r.vector as number[] | undefined,
            });
          }
        } catch (err) {
          // Collection may not exist yet — treat as empty
          logger.debug(`VectorService.search: namespace ${ns} not searchable: ${err}`);
        }
      })
    );

    // Merge and sort by score descending, take top topK
    allResults.sort((a, b) => b.score - a.score);
    return allResults.slice(0, topK);
  }

  /**
   * Hybrid search combining vector + full-text results with MMR and temporal decay.
   */
  async hybridSearch(
    queryVector: number[],
    vectorResults: SearchResult[],
    textResults: SearchResult[],
    config: HybridSearchConfig
  ): Promise<SearchResult[]> {
    const combinedMap = new Map<string | number, SearchResult & { vectorScore?: number; textScore?: number }>();

    // Normalize and add vector results
    const maxVectorScore = Math.max(...vectorResults.map(r => r.score), 0) || 1;
    for (const r of vectorResults) {
      combinedMap.set(r.id, { ...r, vectorScore: r.score / maxVectorScore });
    }

    // Normalize and add text results
    const maxTextScore = Math.max(...textResults.map(r => r.score), 0) || 1;
    for (const r of textResults) {
      const existing = combinedMap.get(r.id);
      if (existing) {
        existing.textScore = r.score / maxTextScore;
      } else {
        combinedMap.set(r.id, { ...r, textScore: r.score / maxTextScore });
      }
    }

    let results = Array.from(combinedMap.values());

    // Calculate final hybrid score
    for (const r of results) {
      const vs = r.vectorScore ?? 0;
      const ts = r.textScore ?? 0;
      r.score = config.vectorWeight * vs + config.textWeight * ts;

      // Apply Temporal Decay
      if (config.temporalDecay?.enabled && r.payload.created_at) {
        const ageInDays = (Date.now() - new Date(r.payload.created_at).getTime()) / (1000 * 60 * 60 * 24);
        const decayFactor = Math.pow(2, -ageInDays / config.temporalDecay.halfLifeDays);
        r.score *= decayFactor;
      }
    }

    // Filter by minScore and sort
    results = results.filter(r => r.score >= config.minScore);
    results.sort((a, b) => b.score - a.score);

    // Apply MMR Re-ranking
    if (config.mmr?.enabled && results.length > 0) {
      // Ensure all candidates have vectors
      const missingVectorIds = results.filter(r => !r.vector).map(r => r.id);
      if (missingVectorIds.length > 0) {
        // Fetch missing vectors from Qdrant
        // Note: Qdrant client retrieve can fetch by IDs
        // For multi-namespace, we might need to check which namespace the result came from.
        // For simplicity, we'll try to fetch from all namespaces or assume they are accessible.
        // Actually, SearchResult has namespace.
        const namespaceGroups = new Map<MemoryNamespace, string[]>();
        for (const r of results) {
          if (!r.vector) {
            const group = namespaceGroups.get(r.namespace) ?? [];
            group.push(String(r.id));
            namespaceGroups.set(r.namespace, group);
          }
        }

        await Promise.all(Array.from(namespaceGroups.entries()).map(async ([ns, ids]) => {
          try {
            const points = await this.client.retrieve(collectionName(ns), {
              ids,
              with_vector: true,
            });
            for (const p of points) {
              const res = results.find(r => r.id === p.id);
              if (res && p.vector) {
                res.vector = p.vector as number[];
              }
            }
          } catch (err) {
            logger.warn(`Failed to retrieve vectors for MMR from ${ns}`, err);
          }
        }));
      }

      results = this.applyMMR(results, queryVector, config.mmr.lambda, config.maxResults);
    } else {
      results = results.slice(0, config.maxResults);
    }

    return results;
  }

  private applyMMR(
    candidates: SearchResult[],
    queryVector: number[],
    lambda: number,
    topK: number
  ): SearchResult[] {
    const selected: SearchResult[] = [];
    const remaining = [...candidates];

    // Select the first one (highest score)
    const first = remaining.shift()!;
    selected.push(first);

    while (selected.length < topK && remaining.length > 0) {
      let bestScore = -Infinity;
      let bestIdx = -1;

      for (let i = 0; i < remaining.length; i++) {
        const candidate = remaining[i]!;
        if (!candidate.vector) continue; // Should have vectors for MMR

        // Max similarity to already selected results
        let maxSim = -Infinity;
        for (const s of selected) {
          if (!s.vector) continue;
          const sim = cosineSimilarity(candidate.vector, s.vector);
          if (sim > maxSim) maxSim = sim;
        }

        // MMR Score = lambda * relevance - (1 - lambda) * max_similarity
        const mmrScore = lambda * candidate.score - (1 - lambda) * maxSim;
        if (mmrScore > bestScore) {
          bestScore = mmrScore;
          bestIdx = i;
        }
      }

      if (bestIdx === -1) break;
      selected.push(remaining.splice(bestIdx, 1)[0]!);
    }

    return selected;
  }

  // ── Delete ─────────────────────────────────────────────────────────────────

  async delete(blockId: string, namespace: MemoryNamespace): Promise<void> {
    const name = collectionName(namespace);
    try {
      await this.client.delete(name, { wait: true, points: [blockId] });
    } catch (err) {
      logger.warn(`VectorService.delete: could not delete from ${name}:`, err);
    }
  }

  // ── Rebuild ────────────────────────────────────────────────────────────────

  /**
   * Re-index all markdown blocks in sourcePath into the given namespace.
   * Used after a git merge to main to rebuild circle/global indexes.
   */
  async rebuildNamespace(
    namespace: MemoryNamespace,
    sourcePath: string,
    store: ScopedMemoryBlockStore,
    agentId: string
  ): Promise<void> {
    const name = collectionName(namespace);
    logger.info(`Rebuilding Qdrant namespace "${name}" from ${sourcePath}`);
    const embedding = EmbeddingService.getInstance();

    // Drop and re-create collection
    try {
      await this.client.deleteCollection(name);
    } catch {
      // may not exist
    }
    await this.ensureCollectionByName(name);

    const blocks = await store.list(agentId);
    let indexed = 0;
    for (const block of blocks) {
      try {
        const vector = await embedding.embed(`${block.title}\n${block.content}`);
        await this.client.upsert(name, {
          wait: true,
          points: [
            {
              id: block.id,
              vector,
              payload: {
                agent_id: block.agentId,
                created_at: block.timestamp,
                tags: block.tags,
                type: block.type,
                title: block.title,
                content: block.content,
                source_file: `${block.agentId}/${block.type}`,
                namespace,
              },
            },
          ],
        });
        indexed++;
      } catch (err) {
        logger.warn(`Failed to index block ${block.id} during rebuild:`, err);
      }
    }
    logger.info(`Rebuilt namespace "${name}": ${indexed} blocks indexed`);
  }

  // ── Stats ──────────────────────────────────────────────────────────────────

  async getCollectionInfo(namespace: MemoryNamespace): Promise<{ vectorCount: number } | null> {
    const name = collectionName(namespace);
    try {
      const info = await this.client.getCollection(name);

      let countValue = 0;
      if (info && typeof info === 'object') {
        if ('vectors_count' in info && typeof info.vectors_count === 'number') {
          countValue = info.vectors_count;
        } else if ('points_count' in info && typeof info.points_count === 'number') {
          countValue = info.points_count;
        }
      }

      return { vectorCount: countValue };
    } catch {
      return null;
    }
  }

  // ── Legacy API (MemoryManager backward compat) ────────────────────────────

  /** @deprecated Use upsert() with explicit namespace. */
  async ensureCollection(vectorSize: number): Promise<void> {
    await this.ensureCollectionByName(this.legacyCollectionName, vectorSize);
  }

  /** @deprecated Use upsert() with explicit namespace. */
  async upsertPoints(
    points: Array<{ id: string | number; vector: number[]; payload: unknown }>
  ): Promise<void> {
    await this.client.upsert(this.legacyCollectionName, {
      wait: true,
      points: points.map((p) => ({
        id: p.id,
        vector: p.vector,
        payload: p.payload as Record<string, unknown>,
      })),
    });
  }

  /** @deprecated Use search() with explicit namespaces. */
  async searchLegacy(
    vector: number[],
    limit = 5,
    filter?: unknown
  ): Promise<Array<{ id: string | number; score: number; payload: unknown }>> {
    const params: Parameters<typeof this.client.search>[1] = { vector, limit, with_payload: true };
    if (filter !== undefined) params.filter = filter as Record<string, unknown>;
    const results = await this.client.search(this.legacyCollectionName, params);
    return results.map((r) => ({ id: r.id, score: r.score, payload: r.payload ?? {} }));
  }

  /** @deprecated */
  async deletePoints(ids: (string | number)[]): Promise<void> {
    await this.client.delete(this.legacyCollectionName, { wait: true, points: ids });
  }
}

// ── Filter helpers ─────────────────────────────────────────────────────────────

function cosineSimilarity(a: number[], b: number[]): number {
  let dotProduct = 0;
  let normA = 0;
  let normB = 0;
  for (let i = 0; i < a.length; i++) {
    dotProduct += a[i]! * b[i]!;
    normA += a[i]! * a[i]!;
    normB += b[i]! * b[i]!;
  }
  return dotProduct / (Math.sqrt(normA) * Math.sqrt(normB));
}

function buildQdrantFilter(filter?: SearchFilter): object | undefined {
  if (!filter) return undefined;
  const must: object[] = [];
  const must_not: object[] = [];

  if (filter.type) {
    must.push({ key: 'type', match: { value: filter.type } });
  }
  if (filter.tags && filter.tags.length > 0) {
    for (const tag of filter.tags) {
      must.push({ key: 'tags', match: { value: tag } });
    }
  }
  if (filter.excludeTags && filter.excludeTags.length > 0) {
    for (const tag of filter.excludeTags) {
      must_not.push({ key: 'tags', match: { value: tag } });
    }
  }
  if (filter.since) {
    must.push({ key: 'created_at', range: { gte: filter.since } });
  }
  if (filter.author) {
    must.push({ key: 'agent_id', match: { value: filter.author } });
  }
  if (filter.minImportance !== undefined) {
    must.push({ key: 'importance', range: { gte: filter.minImportance } });
  }

  if (must.length === 0 && must_not.length === 0) return undefined;
  const result: Record<string, object[]> = {};
  if (must.length > 0) result.must = must;
  if (must_not.length > 0) result.must_not = must_not;
  return result;
}
