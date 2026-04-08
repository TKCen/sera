import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import fs from 'fs';
import { ProviderRegistry } from './ProviderRegistry.js';

// Mock fs
vi.mock('fs', () => ({
  default: {
    existsSync: vi.fn(),
    readFileSync: vi.fn(),
    promises: {
      writeFile: vi.fn(),
    },
  },
}));

// Mock logger to avoid console spam
vi.mock('../lib/logger.js', () => {
  return {
    Logger: class {
      info = vi.fn();
      error = vi.fn();
      warn = vi.fn();
      debug = vi.fn();
    },
  };
});

describe('ProviderRegistry', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    vi.resetModules();
    process.env = { ...originalEnv };
    vi.clearAllMocks();
  });

  afterEach(() => {
    process.env = originalEnv;
    vi.restoreAllMocks();
  });

  describe('constructor / loadSync', () => {
    it('should load config if file exists', () => {
      const mockData = {
        providers: [{ modelName: 'test-model', api: 'openai-completions', provider: 'openai' }],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const registry = new ProviderRegistry('/mock/path.json');

      const config = registry.resolve('test-model');
      expect(config.modelName).toBe('test-model');
      expect(config.api).toBe('openai-completions');
    });

    it('should handle missing file gracefully', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      const registry = new ProviderRegistry('/mock/path.json');
      expect(registry.list()).toHaveLength(0);
    });

    it('should handle malformed JSON gracefully', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue('invalid json');

      const registry = new ProviderRegistry('/mock/path.json');
      expect(registry.list()).toHaveLength(0);
    });
  });

  describe('bootstrapFromEnv', () => {
    it('should register default from env if not in config', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      process.env.LLM_BASE_URL = 'https://api.openai.com';
      process.env.LLM_MODEL = 'env-model';
      process.env.LLM_API_KEY = 'env-key';

      const registry = new ProviderRegistry('/mock/path.json');

      const config = registry.resolve('env-model');
      expect(config.modelName).toBe('env-model');
      expect(config.baseUrl).toBe('https://api.openai.com');
      expect(config.apiKey).toBe('env-key');
    });

    it('should not override if already in config', () => {
      const mockData = {
        providers: [
          {
            modelName: 'env-model',
            api: 'openai-completions',
            provider: 'openai',
            baseUrl: 'https://api.openai.com',
          },
        ],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      process.env.LLM_BASE_URL = 'https://api.anthropic.com';
      process.env.LLM_MODEL = 'env-model';

      const registry = new ProviderRegistry('/mock/path.json');

      const config = registry.resolve('env-model');
      expect(config.baseUrl).toBe('https://api.openai.com'); // Kept existing
    });

    it('should do nothing if env vars missing', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      delete process.env.LLM_BASE_URL;

      const registry = new ProviderRegistry('/mock/path.json');
      expect(registry.list()).toHaveLength(0);
    });
  });

  describe('initDefaultModel / setDefaultModel / getDefaultModel', () => {
    it('should set default from DEFAULT_MODEL env var', () => {
      const mockData = {
        providers: [
          { modelName: 'model1', api: 'openai-completions' },
          { modelName: 'model2', api: 'openai-completions' },
        ],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      process.env.DEFAULT_MODEL = 'model2';

      const registry = new ProviderRegistry('/mock/path.json');
      expect(registry.getDefaultModel()).toBe('model2');
    });

    it('should fallback to first registered if DEFAULT_MODEL is missing or invalid', () => {
      const mockData = {
        providers: [
          { modelName: 'model1', api: 'openai-completions' },
          { modelName: 'model2', api: 'openai-completions' },
        ],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      delete process.env.DEFAULT_MODEL;

      const registry = new ProviderRegistry('/mock/path.json');
      expect(registry.getDefaultModel()).toBe('model1');
    });

    it('should allow setting a new default model explicitly', () => {
      const mockData = {
        providers: [
          { modelName: 'model1', api: 'openai-completions' },
          { modelName: 'model2', api: 'openai-completions' },
        ],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const registry = new ProviderRegistry('/mock/path.json');
      expect(registry.getDefaultModel()).toBe('model1');

      registry.setDefaultModel('model2');
      expect(registry.getDefaultModel()).toBe('model2');
    });

    it('should allow setting an auto-detected model as default', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      const registry = new ProviderRegistry('/mock/path.json');
      registry.setDefaultModel('gpt-4');

      expect(registry.getDefaultModel()).toBe('gpt-4');
    });

    it('should throw if setting default to unregistered and un-detectable model', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      const registry = new ProviderRegistry('/mock/path.json');
      expect(() => registry.setDefaultModel('unknown-model')).toThrow(/Cannot set default model/);
    });
  });

  describe('resolve', () => {
    it('should find registered explicit model', () => {
      const mockData = {
        providers: [{ modelName: 'custom-model', api: 'openai-completions' }],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const registry = new ProviderRegistry('/mock/path.json');

      const config = registry.resolve('custom-model');
      expect(config.modelName).toBe('custom-model');
    });

    it('should auto-detect gpt-4', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const registry = new ProviderRegistry('/mock/path.json');

      const config = registry.resolve('gpt-4-turbo');
      expect(config.api).toBe('openai-completions');
      expect(config.provider).toBe('openai');
    });

    it('should auto-detect claude-3', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const registry = new ProviderRegistry('/mock/path.json');

      const config = registry.resolve('claude-3-opus');
      expect(config.api).toBe('anthropic-messages');
      expect(config.provider).toBe('anthropic');
    });

    it('should auto-detect gemini', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const registry = new ProviderRegistry('/mock/path.json');

      const config = registry.resolve('gemini-1.5-pro');
      expect(config.api).toBe('openai-completions');
      expect(config.provider).toBe('google');
    });

    it('should fallback to default model if requesting "default"', () => {
      const mockData = {
        providers: [{ modelName: 'model1', api: 'openai-completions' }],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const registry = new ProviderRegistry('/mock/path.json');

      const config = registry.resolve('default');
      expect(config.modelName).toBe('model1');
    });

    it('should fallback to default model if requested model is not found', () => {
      const mockData = {
        providers: [{ modelName: 'model1', api: 'openai-completions' }],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const registry = new ProviderRegistry('/mock/path.json');

      // Attempting to resolve un-detectable model
      const config = registry.resolve('unknown-model');
      expect(config.modelName).toBe('model1');
    });

    it('should throw if model not found and no default exists', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const registry = new ProviderRegistry('/mock/path.json');

      expect(() => registry.resolve('unknown-model')).toThrow(/No provider registered/);
    });
  });

  describe('register / unregister', () => {
    it('should register and unregister successfully', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const registry = new ProviderRegistry('/mock/path.json');

      await registry.register({ modelName: 'new-model', api: 'openai-completions' });
      expect(registry.list()).toHaveLength(1);

      const removed = registry.unregister('new-model');
      expect(removed).toBe(true);
      expect(registry.list()).toHaveLength(0);
    });
  });

  describe('registerDynamicModels / unregisterDynamicModels', () => {
    it('should register multiple models for a dynamic provider and remove old ones', () => {
      const mockData = {
        providers: [
          { modelName: 'old-dyn-1', api: 'openai-completions', dynamicProviderId: 'dp1' },
          { modelName: 'manual-model', api: 'openai-completions' },
        ],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const registry = new ProviderRegistry('/mock/path.json');
      expect(registry.list()).toHaveLength(2);

      registry.registerDynamicModels('dp1', [
        { modelName: 'new-dyn-1', api: 'openai-completions' },
        { modelName: 'new-dyn-2', api: 'openai-completions' },
      ]);

      const list = registry.list();
      expect(list).toHaveLength(3); // 1 manual + 2 new dynamic
      expect(list.find((m) => m.modelName === 'old-dyn-1')).toBeUndefined();
      expect(list.find((m) => m.modelName === 'manual-model')).toBeDefined();
      expect(list.find((m) => m.modelName === 'new-dyn-1')?.dynamicProviderId).toBe('dp1');
    });

    it('should unregister all models for a dynamic provider', () => {
      const mockData = {
        providers: [
          { modelName: 'dyn-1', api: 'openai-completions', dynamicProviderId: 'dp1' },
          { modelName: 'dyn-2', api: 'openai-completions', dynamicProviderId: 'dp1' },
          { modelName: 'manual-model', api: 'openai-completions' },
        ],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const registry = new ProviderRegistry('/mock/path.json');
      expect(registry.list()).toHaveLength(3);

      registry.unregisterDynamicModels('dp1');

      const list = registry.list();
      expect(list).toHaveLength(1);
      expect(list[0]?.modelName).toBe('manual-model');
    });
  });

  describe('save', () => {
    it('should write current state to file', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const registry = new ProviderRegistry('/mock/path.json');

      await registry.register({ modelName: 'save-model', api: 'openai-completions' });

      await registry.save();

      expect(fs.promises.writeFile).toHaveBeenCalledWith(
        '/mock/path.json',
        expect.stringContaining('save-model'),
        'utf-8'
      );
    });
  });
});
