import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Must mock fetch before importing EmbeddingService
const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

describe('EmbeddingService', () => {
  let EmbeddingService: typeof import('./embedding.service.js').EmbeddingService;
  let EMBEDDING_VECTOR_SIZE: number;

  beforeEach(async () => {
    vi.resetModules();
    mockFetch.mockReset();
    const mod = await import('./embedding.service.js');
    EmbeddingService = mod.EmbeddingService;
    EMBEDDING_VECTOR_SIZE = mod.EMBEDDING_VECTOR_SIZE;
    // Reset singleton for each test
    (EmbeddingService as any).instance = undefined;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns an embedding vector on success', async () => {
    const fakeVector = Array.from({ length: 768 }, (_, i) => i / 768);
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ embedding: fakeVector }),
    });

    const service = EmbeddingService.getInstance();
    const result = await service.embed('hello world');
    expect(result).toEqual(fakeVector);
    expect(result).toHaveLength(768);
  });

  it('retries on HTTP failure and eventually rejects', async () => {
    // Use a very short backoff by patching the delay constant
    mockFetch.mockResolvedValue({ ok: false, text: async () => 'server error', status: 500 });

    const service = EmbeddingService.getInstance();
    // Patch Math.min to return 0 so retries are instant
    const origMin = Math.min;
    vi.spyOn(Math, 'min').mockReturnValue(0);

    await expect(service.embed('test')).rejects.toThrow();
    expect(service.isAvailable()).toBe(false);

    Math.min = origMin;
  });

  it('marks unavailable after all retries exhausted', async () => {
    mockFetch.mockRejectedValue(new Error('ECONNREFUSED'));

    const service = EmbeddingService.getInstance();
    vi.spyOn(Math, 'min').mockReturnValue(0);

    await expect(service.embed('test')).rejects.toThrow();
    expect(service.isAvailable()).toBe(false);
  });

  it('queues concurrent embed() calls', async () => {
    const fakeVector = new Array(768).fill(0.1);
    mockFetch.mockResolvedValue({ ok: true, json: async () => ({ embedding: fakeVector }) });

    const service = EmbeddingService.getInstance();
    const [r1, r2, r3] = await Promise.all([
      service.embed('a'),
      service.embed('b'),
      service.embed('c'),
    ]);
    expect(r1).toEqual(fakeVector);
    expect(r2).toEqual(fakeVector);
    expect(r3).toEqual(fakeVector);
    expect(mockFetch).toHaveBeenCalledTimes(3);
  });

  it('generateEmbedding is an alias for embed()', async () => {
    const fakeVector = new Array(768).fill(0.5);
    mockFetch.mockResolvedValueOnce({ ok: true, json: async () => ({ embedding: fakeVector }) });
    const service = EmbeddingService.getInstance();
    const result = await service.generateEmbedding('alias test');
    expect(result).toEqual(fakeVector);
  });
});
