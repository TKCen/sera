import { QdrantClient } from '@qdrant/js-client-rest';
import { pool } from '../lib/database.js';
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
  vector?: number[] | undefined;
  timestamp?: string | undefined;
  textScore?: number | undefined;
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
  /** @internal exported for testing */
  public readonly normalizeScores = normalizeScores;
  /** @internal exported for testing */
  public readonly applyTemporalDecay = applyTemporalDecay;
  /** @internal exported for testing */
  public readonly reRankWithMMR = reRankWithMMR;

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

    // Synchronize with PostgreSQL memory_blocks table
    try {
      await pool.query(
        `INSERT INTO memory_blocks (id, agent_id, namespace, type, title, content, tags, importance, created_at, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (id) DO UPDATE SET
           agent_id = EXCLUDED.agent_id,
           namespace = EXCLUDED.namespace,
           type = EXCLUDED.type,
           title = EXCLUDED.title,
           content = EXCLUDED.content,
           tags = EXCLUDED.tags,
           importance = EXCLUDED.importance,
           created_at = EXCLUDED.created_at,
           metadata = EXCLUDED.metadata`,
        [
          blockId,
          payload.agent_id,
          namespace,
          payload.type,
          payload.title || '',
          payload.content || '',
          payload.tags || [],
          (payload['importance'] as number) || 3,
          payload.created_at || new Date().toISOString(),
          JSON.stringify(payload),
        ]
      );
    } catch (err) {
      logger.error(`Failed to synchronize block ${blockId} to PostgreSQL:`, err);
    }
  }

  // ── Search ─────────────────────────────────────────────────────────────────

  /**
   * Hybrid search across multiple namespaces.
   * Combines vector similarity from Qdrant and full-text ranking from PostgreSQL.
   */
  async search(
    namespaces: MemoryNamespace[],
    queryVector: number[],
    topK: number,
    filter?: SearchFilter,
    config?: import('../agents/manifest/types.js').MemorySearchConfig,
    queryText?: string
  ): Promise<SearchResult[]> {
    const vectorWeight = config?.vectorWeight ?? 0.7;
    const textWeight = config?.textWeight ?? 0.3;
    const minScore = config?.minScore ?? 0.35;
    const mmrEnabled = config?.mmr?.enabled ?? true;
    const mmrLambda = config?.mmr?.lambda ?? 0.7;
    const mmrMultiplier = config?.mmr?.candidateMultiplier ?? 4;
    const temporalEnabled = config?.temporalDecay?.enabled ?? true;
    const temporalHalfLife = config?.temporalDecay?.halfLifeDays ?? 30;

    const candidateLimit = mmrEnabled ? topK * mmrMultiplier : topK * 2;
    const perNamespace = Math.max(Math.ceil(candidateLimit / namespaces.length), topK);

    const vectorResults: SearchResult[] = [];
    const textResults: SearchResult[] = [];

    // 1. Vector Search
    await Promise.all(
      namespaces.map(async (ns) => {
        const name = collectionName(ns);
        try {
          const qdrantFilter = buildQdrantFilter(filter);
          const searchParams: Parameters<typeof this.client.search>[1] = {
            vector: queryVector,
            limit: perNamespace,
            with_payload: true,
            with_vector: true,
          };
          if (qdrantFilter) {
            searchParams.filter = qdrantFilter as Record<string, unknown>;
          }
          const results = await this.client.search(name, searchParams);
          for (const r of results) {
            const res: SearchResult = {
              id: r.id,
              score: r.score,
              payload: (r.payload ?? {}) as VectorPayload,
              namespace: ns,
            };
            if (Array.isArray(r.vector) && typeof r.vector[0] === 'number') {
              res.vector = r.vector as number[];
            }
            if ((r.payload as VectorPayload)?.created_at) {
              res.timestamp = (r.payload as VectorPayload).created_at;
            }
            vectorResults.push(res);
          }
        } catch (err) {
          logger.debug(`VectorService.search: vector namespace ${ns} not searchable: ${err}`);
        }
      })
    );

    const namespacesArray = namespaces as string[];

    // 2. Full-text Search (PostgreSQL)
    if (queryText && textWeight > 0) {
      try {
        const { rows } = await pool.query(
          `SELECT id, agent_id, namespace, type, title, content, tags, importance, created_at, metadata,
                  ts_rank_cd(tsv, plainto_tsquery('english', $1)) as rank
           FROM memory_blocks
           WHERE namespace = ANY($2)
             AND tsv @@ plainto_tsquery('english', $1)
           ORDER BY rank DESC
           LIMIT $3`,
          [queryText, namespacesArray, candidateLimit]
        );

        for (const row of rows) {
          textResults.push({
            id: row.id,
            score: row.rank,
            payload: {
              ...((row.metadata as VectorPayload) ?? {}),
              agent_id: row.agent_id,
              created_at:
                row.created_at instanceof Date ? row.created_at.toISOString() : row.created_at,
              tags: row.tags,
              type: row.type,
              title: row.title,
              content: row.content,
            },
            namespace: row.namespace as MemoryNamespace,
            timestamp:
              row.created_at instanceof Date ? row.created_at.toISOString() : row.created_at,
          });
        }
      } catch (err) {
        logger.error('Full-text search failed:', err);
      }
    }

    // 3. Normalization and Hybrid Combination
    const normVector = normalizeScores(vectorResults);
    const normText = normalizeScores(textResults);

    const mergedMap = new Map<string | number, SearchResult>();

    for (const r of normVector) {
      mergedMap.set(r.id, { ...r, score: r.score * vectorWeight });
    }

    for (const r of normText) {
      const existing = mergedMap.get(r.id);
      if (existing) {
        existing.score += r.score * textWeight;
        existing.textScore = r.score;
      } else {
        mergedMap.set(r.id, { ...r, score: r.score * textWeight, textScore: r.score });
      }
    }

    // 3.5 Fetch missing vectors for MMR quality (if enabled)
    if (mmrEnabled) {
      const missingVectorIds = Array.from(mergedMap.values())
        .filter((r) => !r.vector)
        .map((r) => r.id);

      if (missingVectorIds.length > 0) {
        for (const ns of namespaces) {
          try {
            const points = await this.client.retrieve(collectionName(ns), {
              ids: missingVectorIds,
              with_vector: true,
            });
            for (const p of points) {
              const r = mergedMap.get(p.id);
              if (r && Array.isArray(p.vector) && typeof p.vector[0] === 'number') {
                r.vector = p.vector as number[];
              }
            }
          } catch {
            // Ignore — Qdrant may not have the point or collection
          }
        }
      }
    }

    let results = Array.from(mergedMap.values());

    // 4. Temporal Decay
    if (temporalEnabled) {
      for (const r of results) {
        if (r.timestamp) {
          r.score = applyTemporalDecay(r.score, r.timestamp, temporalHalfLife);
        }
      }
    }

    // 5. Final Filtering and Sorting
    results = results.filter((r) => r.score >= minScore);
    results.sort((a, b) => b.score - a.score);

    // 6. MMR Re-ranking
    if (mmrEnabled) {
      results = reRankWithMMR(results, topK, mmrLambda);
    } else {
      results = results.slice(0, topK);
    }

    return results;
  }

  // ── Delete ─────────────────────────────────────────────────────────────────

  async delete(blockId: string, namespace: MemoryNamespace): Promise<void> {
    const name = collectionName(namespace);
    try {
      await this.client.delete(name, { wait: true, points: [blockId] });
      // Delete from PostgreSQL
      await pool.query('DELETE FROM memory_blocks WHERE id = $1', [blockId]);
    } catch (err) {
      logger.warn(`VectorService.delete: could not delete ${blockId} from ${name} or PG:`, err);
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

    // Clear matching PostgreSQL entries for this namespace
    try {
      await pool.query('DELETE FROM memory_blocks WHERE namespace = $1', [namespace]);
    } catch (err) {
      logger.warn(`Failed to clear namespace ${namespace} from PostgreSQL:`, err);
    }

    const blocks = await store.list(agentId);
    let indexed = 0;
    for (const block of blocks) {
      try {
        const vector = await embedding.embed(`${block.title}\n${block.content}`);
        await this.upsert(block.id, namespace, vector, {
          agent_id: block.agentId,
          created_at: block.timestamp,
          tags: block.tags,
          type: block.type,
          title: block.title,
          content: block.content,
          source_file: `${block.agentId}/${block.type}`,
          namespace,
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

// ── Hybrid Search Helpers ─────────────────────────────────────────────────────

function normalizeScores(results: SearchResult[]): SearchResult[] {
  if (results.length === 0) return [];
  const scores = results.map((r) => r.score);
  const min = Math.min(...scores);
  const max = Math.max(...scores);
  const range = max - min;

  return results.map((r) => ({
    ...r,
    score: range === 0 ? 1 : (r.score - min) / range,
  }));
}

function applyTemporalDecay(score: number, createdAt: string, halfLifeDays: number = 30): number {
  const now = new Date();
  const created = new Date(createdAt);
  const ageInDays = (now.getTime() - created.getTime()) / (1000 * 60 * 60 * 24);
  const decayFactor = Math.pow(2, -ageInDays / halfLifeDays);
  return score * decayFactor;
}

/**
 * Maximal Marginal Relevance (MMR) re-ranking.
 * @param candidates - Input results with vectors and normalized scores.
 * @param topK - Number of results to select.
 * @param lambda - Diversity factor (1.0 = pure relevance, 0.0 = pure diversity).
 */
function reRankWithMMR(
  candidates: SearchResult[],
  topK: number,
  lambda: number = 0.7
): SearchResult[] {
  if (candidates.length <= 1 || topK <= 1) return candidates.slice(0, topK);

  const selected: SearchResult[] = [];
  const remaining = [...candidates];

  // 1. Select the most relevant candidate first
  const first = remaining.shift()!;
  selected.push(first);

  // 2. Iteratively select remaining slots
  while (selected.length < topK && remaining.length > 0) {
    let bestScore = -Infinity;
    let bestIdx = -1;

    for (let i = 0; i < remaining.length; i++) {
      const candidate = remaining[i]!;

      // Calculate max similarity with already selected results
      let maxSim = 0;
      if (candidate.vector) {
        for (const sel of selected) {
          if (!sel.vector) continue;
          const sim = cosineSimilarity(candidate.vector, sel.vector);
          if (sim > maxSim) maxSim = sim;
        }
      }

      // MMR formula: lambda * relevance - (1 - lambda) * max_similarity
      const score = lambda * candidate.score - (1 - lambda) * maxSim;

      if (score > bestScore) {
        bestScore = score;
        bestIdx = i;
      }
    }

    if (bestIdx === -1) break;
    selected.push(remaining.splice(bestIdx, 1)[0]!);
  }

  return selected;
}

function cosineSimilarity(v1: number[], v2: number[]): number {
  if (v1.length !== v2.length) return 0;
  let dotProduct = 0;
  let norm1 = 0;
  let norm2 = 0;
  for (let i = 0; i < v1.length; i++) {
    dotProduct += v1[i]! * v2[i]!;
    norm1 += v1[i]! * v1[i]!;
    norm2 += v2[i]! * v2[i]!;
  }
  const norm = Math.sqrt(norm1) * Math.sqrt(norm2);
  return norm === 0 ? 0 : dotProduct / norm;
}
