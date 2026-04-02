import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  updateProvider,
  getProviders,
  deleteProvider,
  createProvider,
  updateProviderConfig,
  testProvider,
  setDefaultModel,
  getDefaultModel
} from './providers';

// Mock global fetch
const fetchMock = vi.fn();
global.fetch = fetchMock;

describe('providers api', () => {
  beforeEach(() => {
    fetchMock.mockReset();
  });

  describe('updateProvider', () => {
    it('sends a PUT request to the correct endpoint with the provider config', async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ success: true }),
      });

      const providerId = 'test-provider';
      const config = { baseUrl: 'https://api.example.com', model: 'gpt-4' };

      const result = await updateProvider(providerId, config);

      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining(`/api/providers/${providerId}`),
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify(config),
        })
      );
      expect(result).toEqual({ success: true });
    });

    it('correctly encodes the provider ID in the URL', async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ success: true }),
      });

      const providerId = 'provider with spaces';
      await updateProvider(providerId, {});

      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining(`/api/providers/provider%20with%20spaces`),
        expect.any(Object)
      );
    });
  });

  describe('getProviders', () => {
    it('sends a GET request to the list endpoint', async () => {
      const mockProviders = { providers: [{ modelName: 'gpt-4', api: 'openai' }] };
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => mockProviders,
      });

      const result = await getProviders();

      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining('/api/providers/list'),
        expect.objectContaining({
          headers: expect.objectContaining({
            'Content-Type': 'application/json',
          }),
        })
      );
      expect(result).toEqual(mockProviders);
    });
  });

  describe('deleteProvider', () => {
    it('sends a DELETE request to the correct endpoint', async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ success: true }),
      });

      const providerId = 'to-delete';
      const result = await deleteProvider(providerId);

      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining(`/api/providers/${providerId}`),
        expect.objectContaining({
          method: 'DELETE',
        })
      );
      expect(result).toEqual({ success: true });
    });
  });

  describe('createProvider', () => {
    it('sends a POST request with the new provider payload', async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ success: true }),
      });

      const payload = {
        name: 'new-provider',
        type: 'cloud' as const,
        modelId: 'gpt-3.5-turbo'
      };
      const result = await createProvider(payload);

      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining('/api/providers'),
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify(payload),
        })
      );
      expect(result).toEqual({ success: true });
    });
  });

  describe('updateProviderConfig', () => {
    it('sends a PATCH request with config overrides', async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ success: true }),
      });

      const modelName = 'gpt-4';
      const config = { contextWindow: 8192 };
      const result = await updateProviderConfig(modelName, config);

      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining(`/api/providers/${modelName}`),
        expect.objectContaining({
          method: 'PATCH',
          body: JSON.stringify(config),
        })
      );
      expect(result).toEqual({ success: true });
    });
  });

  describe('testProvider', () => {
    it('sends a POST request to the test endpoint', async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ success: true, provider: 'test' }),
      });

      const providerId = 'test-id';
      const result = await testProvider(providerId);

      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining(`/api/providers/${providerId}/test`),
        expect.objectContaining({
          method: 'POST',
        })
      );
      expect(result).toEqual({ success: true, provider: 'test' });
    });
  });

  describe('default model functions', () => {
    it('getDefaultModel sends a GET request', async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ defaultModel: 'gpt-4' }),
      });

      const result = await getDefaultModel();
      expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/providers/default-model'), expect.any(Object));
      expect(result).toEqual({ defaultModel: 'gpt-4' });
    });

    it('setDefaultModel sends a PUT request', async () => {
      fetchMock.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ success: true, defaultModel: 'gpt-4' }),
      });

      const result = await setDefaultModel('gpt-4');
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining('/api/providers/default-model'),
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify({ modelName: 'gpt-4' })
        })
      );
      expect(result).toEqual({ success: true, defaultModel: 'gpt-4' });
    });
  });
});
