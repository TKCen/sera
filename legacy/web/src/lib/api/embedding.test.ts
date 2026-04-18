import { describe, it, expect, vi, beforeEach } from 'vitest';
import { request } from './client';
import {
  getEmbeddingConfig,
  updateEmbeddingConfig,
  testEmbeddingConfig,
  getEmbeddingModels,
  getEmbeddingStatus,
  getKnownEmbeddingModels,
  EmbeddingConfig,
} from './embedding';

vi.mock('./client', () => ({
  request: vi.fn(),
}));

describe('embedding api', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  const mockConfig: EmbeddingConfig = {
    provider: 'openai',
    model: 'text-embedding-3-small',
    baseUrl: 'https://api.openai.com/v1',
    dimension: 1536,
  };

  describe('getEmbeddingConfig', () => {
    it('returns configuration when request is successful', async () => {
      vi.mocked(request).mockResolvedValueOnce(mockConfig);

      const result = await getEmbeddingConfig();

      expect(request).toHaveBeenCalledWith('/embedding/config');
      expect(result).toEqual(mockConfig);
    });

    it('propagates errors when request fails', async () => {
      const error = new Error('API Error');
      vi.mocked(request).mockRejectedValueOnce(error);

      await expect(getEmbeddingConfig()).rejects.toThrow('API Error');
    });
  });

  describe('updateEmbeddingConfig', () => {
    it('sends PUT request with config and returns result', async () => {
      const mockResult = {
        config: mockConfig,
        testResult: { ok: true, latencyMs: 100 },
      };
      vi.mocked(request).mockResolvedValueOnce(mockResult);

      const result = await updateEmbeddingConfig(mockConfig);

      expect(request).toHaveBeenCalledWith('/embedding/config', {
        method: 'PUT',
        body: JSON.stringify(mockConfig),
      });
      expect(result).toEqual(mockResult);
    });
  });

  describe('testEmbeddingConfig', () => {
    it('sends POST request with config and returns test result', async () => {
      const mockTestResult = { ok: true, latencyMs: 150 };
      vi.mocked(request).mockResolvedValueOnce(mockTestResult);

      const result = await testEmbeddingConfig(mockConfig);

      expect(request).toHaveBeenCalledWith('/embedding/test', {
        method: 'POST',
        body: JSON.stringify(mockConfig),
      });
      expect(result).toEqual(mockTestResult);
    });
  });

  describe('getEmbeddingModels', () => {
    it('calls /embedding/models without query params when not provided', async () => {
      const mockModels = { models: [{ id: 'model-1' }] };
      vi.mocked(request).mockResolvedValueOnce(mockModels);

      const result = await getEmbeddingModels();

      expect(request).toHaveBeenCalledWith('/embedding/models');
      expect(result).toEqual(mockModels);
    });

    it('calls /embedding/models with query params when provided', async () => {
      const mockModels = { models: [{ id: 'model-1' }] };
      vi.mocked(request).mockResolvedValueOnce(mockModels);

      await getEmbeddingModels('ollama', 'http://localhost:11434');

      expect(request).toHaveBeenCalledWith(
        '/embedding/models?provider=ollama&baseUrl=http%3A%2F%2Flocalhost%3A11434'
      );
    });
  });

  describe('getEmbeddingStatus', () => {
    it('returns status when request is successful', async () => {
      const mockStatus = {
        available: true,
        configured: true,
        provider: 'openai',
        model: 'text-embedding-3-small',
        dimension: 1536,
        baseUrl: 'https://api.openai.com/v1',
      };
      vi.mocked(request).mockResolvedValueOnce(mockStatus);

      const result = await getEmbeddingStatus();

      expect(request).toHaveBeenCalledWith('/embedding/status');
      expect(result).toEqual(mockStatus);
    });
  });

  describe('getKnownEmbeddingModels', () => {
    it('returns known models map when request is successful', async () => {
      const mockKnownModels = {
        'text-embedding-3-small': {
          provider: 'openai' as const,
          dimension: 1536,
          description: 'OpenAI small model',
        },
      };
      vi.mocked(request).mockResolvedValueOnce(mockKnownModels);

      const result = await getKnownEmbeddingModels();

      expect(request).toHaveBeenCalledWith('/embedding/known-models');
      expect(result).toEqual(mockKnownModels);
    });
  });
});
