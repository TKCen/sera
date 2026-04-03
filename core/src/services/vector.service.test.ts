import { describe, it, expect, vi, beforeEach } from 'vitest';
import { VectorService, type SearchResult, type HybridSearchConfig } from './vector.service.js';

describe('VectorService Hybrid Search', () => {
  let vectorService: VectorService;

  beforeEach(() => {
    vectorService = new VectorService();
  });

  it('should combine and normalize scores from vector and text search', async () => {
    const queryVector = [0.1, 0.2];
    const vectorResults: SearchResult[] = [
      {
        id: '1',
        score: 0.8,
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
      {
        id: '2',
        score: 0.4,
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
    ];
    const textResults: SearchResult[] = [
      {
        id: '2',
        score: 10.0,
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
      {
        id: '3',
        score: 5.0,
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
    ];

    const config: HybridSearchConfig = {
      vectorWeight: 0.5,
      textWeight: 0.5,
      minScore: 0.1,
      maxResults: 5,
    };

    const results = await vectorService.hybridSearch(
      queryVector,
      vectorResults,
      textResults,
      config
    );

    // ID 2: Vector Score 0.4 (norm 0.5), Text Score 10.0 (norm 1.0) -> Hybrid Score 0.5*0.5 + 0.5*1.0 = 0.75
    // ID 1: Vector Score 0.8 (norm 1.0), Text Score 0 (norm 0) -> Hybrid Score 0.5*1.0 + 0.5*0 = 0.5
    // ID 3: Vector Score 0 (norm 0), Text Score 5.0 (norm 0.5) -> Hybrid Score 0.5*0 + 0.5*0.5 = 0.25

    expect(results[0]!.id).toBe('2');
    expect(results[0]!.score).toBeCloseTo(0.75);
    expect(results[1]!.id).toBe('1');
    expect(results[1]!.score).toBeCloseTo(0.5);
    expect(results[2]!.id).toBe('3');
    expect(results[2]!.score).toBeCloseTo(0.25);
  });

  it('should apply temporal decay', async () => {
    const queryVector = [0.1, 0.2];
    const now = new Date();
    const thirtyDaysAgo = new Date();
    thirtyDaysAgo.setDate(now.getDate() - 30);

    const vectorResults: SearchResult[] = [
      {
        id: 'recent',
        score: 1.0,
        payload: { created_at: now.toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
      {
        id: 'old',
        score: 1.0,
        payload: { created_at: thirtyDaysAgo.toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
    ];

    const config: HybridSearchConfig = {
      vectorWeight: 1.0,
      textWeight: 0.0,
      minScore: 0.1,
      maxResults: 5,
      temporalDecay: {
        enabled: true,
        halfLifeDays: 30,
      },
    };

    const results = await vectorService.hybridSearch(queryVector, vectorResults, [], config);

    expect(results[0]!.id).toBe('recent');
    expect(results[0]!.score).toBeCloseTo(1.0);
    expect(results[1]!.id).toBe('old');
    expect(results[1]!.score).toBeCloseTo(0.5); // 1.0 * 2^(-30/30) = 0.5
  });

  it('should apply MMR re-ranking', async () => {
    const queryVector = [1, 0];
    const vectorResults: SearchResult[] = [
      {
        id: '1',
        score: 1.0,
        vector: [1, 0],
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
      {
        id: '2',
        score: 0.9,
        vector: [0.99, 0.01],
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
      {
        id: '3',
        score: 0.8,
        vector: [0, 1],
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
    ];

    const config: HybridSearchConfig = {
      vectorWeight: 1.0,
      textWeight: 0.0,
      minScore: 0.1,
      maxResults: 2,
      mmr: {
        enabled: true,
        lambda: 0.5,
        candidateMultiplier: 2,
      },
    };

    const results = await vectorService.hybridSearch(queryVector, vectorResults, [], config);

    // Selected 1 first.
    // Candidates: 2, 3.
    // ID 2 Sim to 1: ~1. MMR Score: 0.5 * 0.9 - 0.5 * 1 = -0.05
    // ID 3 Sim to 1: 0. MMR Score: 0.5 * 0.8 - 0.5 * 0 = 0.4
    // So ID 3 should be selected second even if it has lower relevance than ID 2.

    expect(results).toHaveLength(2);
    expect(results[0]!.id).toBe('1');
    expect(results[1]!.id).toBe('3');
  });

  it('should fetch missing vectors for MMR', async () => {
    const queryVector = [1, 0];
    const vectorResults: SearchResult[] = [
      {
        id: '1',
        score: 1.0,
        vector: [1, 0],
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
    ];
    const textResults: SearchResult[] = [
      {
        id: '2',
        score: 0.9,
        payload: { created_at: new Date().toISOString() } as any,
        namespace: 'personal:agent' as any,
      },
    ];

    const mockRetrieve = vi.fn().mockResolvedValue([{ id: '2', vector: [0, 1] }]);
    (vectorService as any).client = {
      retrieve: mockRetrieve,
    };

    const config: HybridSearchConfig = {
      vectorWeight: 0.5,
      textWeight: 0.5,
      minScore: 0.1,
      maxResults: 2,
      mmr: {
        enabled: true,
        lambda: 0.5,
        candidateMultiplier: 2,
      },
    };

    const results = await vectorService.hybridSearch(
      queryVector,
      vectorResults,
      textResults,
      config
    );

    expect(mockRetrieve).toHaveBeenCalled();
    expect(results).toHaveLength(2);
    expect(results[1]!.id).toBe('2');
    expect(results[1]!.vector).toEqual([0, 1]);
  });
});
