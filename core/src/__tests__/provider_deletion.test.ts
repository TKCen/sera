
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { ProviderRegistry } from '../llm/ProviderRegistry.js';
import { LlmRouter } from '../llm/LlmRouter.js';
import { CircuitBreakerService } from '../llm/CircuitBreakerService.js';
import fs from 'fs';

vi.mock('fs', () => {
  const mockReadFileSync = vi.fn().mockImplementation((path: string) => {
    if (path === '/fake/path.json') {
      return JSON.stringify({
        providers: [
          {
            modelName: 'static-model',
            api: 'openai-completions',
            provider: 'openai',
            enabled: true,
          },
          {
            modelName: 'dynamic-model',
            api: 'openai-completions',
            provider: 'openai',
            dynamicProviderId: 'dp-1',
            enabled: true,
          },
        ],
      });
    }
    throw new Error('File not found: ' + path);
  });

  const mockExistsSync = vi.fn().mockImplementation((path: string) => {
    if (path === '/fake/path.json') return true;
    return false;
  });

  const mockWriteFile = vi.fn().mockResolvedValue(undefined);

  return {
    default: {
      promises: {
        writeFile: mockWriteFile,
      },
      readFileSync: mockReadFileSync,
      existsSync: mockExistsSync,
    },
    promises: {
      writeFile: mockWriteFile,
    },
    readFileSync: mockReadFileSync,
    existsSync: mockExistsSync,
  };
});

describe('Provider Deletion/Disabling', () => {
  let registry: ProviderRegistry;
  let router: LlmRouter;
  let cbService: CircuitBreakerService;

  beforeEach(() => {
    vi.clearAllMocks();
    registry = new ProviderRegistry('/fake/path.json');
    router = new LlmRouter(registry);
    cbService = new CircuitBreakerService(router);
  });

  it('should disable static providers instead of deleting them', async () => {
    const modelName = 'static-model';

    // Initial state
    const models = await router.listModels();
    expect(models.find(m => m.modelName === modelName)?.enabled).toBe(true);

    // Unregister (static)
    await router.deleteModel(modelName);

    // Verify in registry
    const updatedModels = await router.listModels();
    const staticModel = updatedModels.find(m => m.modelName === modelName);
    expect(staticModel).toBeDefined();
    expect(staticModel?.enabled).toBe(false);

    // Verify resolve throws
    expect(() => registry.resolve(modelName)).toThrow(`Provider for model '${modelName}' is disabled.`);
  });

  it('should fully delete dynamic providers', async () => {
    const modelName = 'dynamic-model';

    // Initial state
    const models = await router.listModels();
    expect(models.find(m => m.modelName === modelName)).toBeDefined();

    // Unregister (dynamic)
    await router.deleteModel(modelName);

    // Verify in registry
    const updatedModels = await router.listModels();
    const dynamicModel = updatedModels.find(m => m.modelName === modelName);
    expect(dynamicModel).toBeUndefined();
  });

  it('should remove circuit breaker when requested', () => {
    // Trigger breaker creation
    (cbService as any).getBreakerForProvider('openai');
    expect((cbService as any).breakers.has('openai')).toBe(true);

    // Remove breaker
    cbService.removeBreaker('openai');
    expect((cbService as any).breakers.has('openai')).toBe(false);
  });
});
