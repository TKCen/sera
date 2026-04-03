import { describe, it, expect, vi } from 'vitest';
import { VectorService } from './vector.service.js';
import type { SearchResult, VectorPayload } from './vector.service.js';
import type { MemorySearchConfig } from '../agents/manifest/types.js';

// Mock database pool
vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(async () => {
      const rows = [];
      for (let i = 0; i < 100; i++) {
        rows.push({
          id: `pg-${i}`,
          agent_id: 'test',
          namespace: 'global',
          type: 'fact',
          title: `PostgreSQL Result ${i}`,
          content: 'Some content',
          tags: [],
          importance: 3,
          created_at: new Date().toISOString(),
          metadata: {},
          rank: Math.random(),
        });
      }
      return { rows };
    }),
  },
}));

// Mock EmbeddingService
vi.mock('./embedding.service.js', () => ({
  EmbeddingService: {
    getInstance: () => ({
      isAvailable: () => true,
      embed: async (_text: string) => new Array(768).fill(0.1),
    }),
  },
  getEmbeddingDimension: () => 768,
}));

describe('VectorService Hybrid Search Logic', () => {
  const service = new VectorService('test');

  it('should apply temporal decay correctly', () => {
    const now = new Date();
    const oldDate = new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000); // 30 days ago

    const decayedScore = (service as any).applyTemporalDecay(1.0, oldDate.toISOString(), 30);
    expect(decayedScore).toBeCloseTo(0.5, 5); // Half-life of 30 days should halve the score
  });

  it('should normalize scores correctly', () => {
    const results: SearchResult[] = [
      { id: 1, score: 10, payload: {} as unknown as VectorPayload, namespace: 'global' },
      { id: 2, score: 5, payload: {} as unknown as VectorPayload, namespace: 'global' },
      { id: 3, score: 0, payload: {} as unknown as VectorPayload, namespace: 'global' },
    ];

    const normalized = (service as any).normalizeScores(results);
    expect(normalized[0]?.score).toBe(1);
    expect(normalized[1]?.score).toBe(0.5);
    expect(normalized[2]?.score).toBe(0);
  });

  it('should re-rank with MMR correctly', () => {
    const v1 = [1, 0, 0];
    const v2 = [0.99, 0.01, 0]; // Very similar to v1
    const v3 = [0, 1, 0]; // Very different from v1

    const candidates: SearchResult[] = [
      {
        id: '1',
        score: 1.0,
        vector: v1,
        payload: {} as unknown as VectorPayload,
        namespace: 'global',
      },
      {
        id: '2',
        score: 0.9,
        vector: v2,
        payload: {} as unknown as VectorPayload,
        namespace: 'global',
      },
      {
        id: '3',
        score: 0.8,
        vector: v3,
        payload: {} as unknown as VectorPayload,
        namespace: 'global',
      },
    ];

    // With lambda = 0.5, v3 should be preferred over v2 because it's more diverse
    const reranked = (service as any).reRankWithMMR(candidates, 2, 0.5);

    expect(reranked[0]?.id).toBe('1');
    expect(reranked[1]?.id).toBe('3'); // Diversity wins over relevance
  });

  it('should complete hybrid search within 100ms', async () => {
    // Mock Qdrant client
    (service as any).client = {
      search: vi.fn(async () => {
        const results = [];
        for (let i = 0; i < 100; i++) {
          results.push({
            id: `qd-${i}`,
            score: Math.random(),
            payload: { created_at: new Date().toISOString() },
            vector: new Array(768).fill(Math.random()),
          });
        }
        return results;
      }),
      getCollections: vi.fn(async () => ({ collections: [{ name: 'memory_global' }] })),
    };

    const start = performance.now();
    const iterations = 50;

    const config: MemorySearchConfig = {
      vectorWeight: 0.7,
      textWeight: 0.3,
      minScore: 0.35,
      maxResults: 10,
      mmr: { enabled: true, lambda: 0.7, candidateMultiplier: 4 },
      temporalDecay: { enabled: true, halfLifeDays: 30 },
    };

    for (let i = 0; i < iterations; i++) {
      await service.search(
        ['global'],
        new Array(768).fill(0.1),
        10,
        {},
        config,
        'test query'
      );
    }

    const end = performance.now();
    const avg = (end - start) / iterations;

    console.log(`Average search time: ${avg.toFixed(2)}ms`);
    expect(avg).toBeLessThan(100);
  });
});
