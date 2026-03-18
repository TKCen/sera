import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ProviderFactory } from './ProviderFactory.js';
import { OpenAIProvider } from './OpenAIProvider.js';
import { config } from '../config.js';

// Mock dependencies
vi.mock('../config.js', () => ({
  config: {
    getProviderConfig: vi.fn(),
    llm: {
      baseUrl: 'http://global-fallback:1234/v1',
      apiKey: 'global-key',
      model: 'global-model'
    }
  }
}));

vi.mock('./OpenAIProvider.js', () => {
  return {
    OpenAIProvider: vi.fn().mockImplementation((config) => {
      return {
        configOverride: config,
        chat: vi.fn(),
        chatStream: vi.fn()
      };
    })
  };
});

describe('ProviderFactory', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('createFromModelConfig', () => {
    it('should create an OpenAIProvider using the found provider config', () => {
      const mockProviderConfig = {
        baseUrl: 'http://custom-provider:8000/v1',
        apiKey: 'custom-key'
      };
      vi.mocked(config.getProviderConfig).mockReturnValue(mockProviderConfig);

      const modelConfig = {
        provider: 'custom-id',
        name: 'custom-model',
        temperature: 0.5
      };

      const provider = ProviderFactory.createFromModelConfig(modelConfig) as any;

      expect(config.getProviderConfig).toHaveBeenCalledWith('custom-id');
      expect(OpenAIProvider).toHaveBeenCalledWith({
        baseUrl: 'http://custom-provider:8000/v1',
        apiKey: 'custom-key',
        model: 'custom-model',
        temperature: 0.5
      });
      expect(provider.configOverride.baseUrl).toBe('http://custom-provider:8000/v1');
    });

    it('should fallback to global config if provider config is not found', () => {
      vi.mocked(config.getProviderConfig).mockReturnValue(undefined);

      const modelConfig = {
        provider: 'non-existent',
        name: 'requested-model'
      };

      ProviderFactory.createFromModelConfig(modelConfig);

      expect(OpenAIProvider).toHaveBeenCalledWith({
        baseUrl: 'http://global-fallback:1234/v1',
        apiKey: 'global-key',
        model: 'requested-model'
      });
    });

    it('should use default apiKey if provider config exists but has no apiKey', () => {
      vi.mocked(config.getProviderConfig).mockReturnValue({
        baseUrl: 'http://no-key-provider:1234/v1'
      });

      const modelConfig = {
        provider: 'no-key-id',
        name: 'some-model'
      };

      ProviderFactory.createFromModelConfig(modelConfig);

      expect(OpenAIProvider).toHaveBeenCalledWith({
        baseUrl: 'http://no-key-provider:1234/v1',
        apiKey: 'not-needed',
        model: 'some-model'
      });
    });
  });

  describe('createFromManifest', () => {
    it('should call createFromModelConfig with manifest.model', () => {
      const manifest = {
        model: {
          provider: 'manifest-provider',
          name: 'manifest-model',
          temperature: 0.9
        }
      } as any;

      const spy = vi.spyOn(ProviderFactory, 'createFromModelConfig');
      ProviderFactory.createFromManifest(manifest);

      expect(spy).toHaveBeenCalledWith(manifest.model);
    });
  });

  describe('createDefault', () => {
    it('should create an OpenAIProvider with no arguments', () => {
      ProviderFactory.createDefault();
      expect(OpenAIProvider).toHaveBeenCalledWith();
    });
  });
});
