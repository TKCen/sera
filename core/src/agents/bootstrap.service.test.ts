import { describe, it, expect, vi, beforeEach } from 'vitest';
import { BootstrapService } from './bootstrap.service.js';
import type { AgentRegistry } from './registry.service.js';
import type { ResourceImporter } from './importer.service.js';

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

describe('BootstrapService', () => {
  let mockRegistry: Partial<AgentRegistry>;
  let mockImporter: Partial<ResourceImporter>;
  let bootstrapService: BootstrapService;

  beforeEach(() => {
    mockRegistry = {
      listInstances: vi.fn(),
      getTemplate: vi.fn(),
      createInstance: vi.fn(),
    };
    mockImporter = {
      importAll: vi.fn(),
    };
    bootstrapService = new BootstrapService(
      mockRegistry as AgentRegistry,
      mockImporter as ResourceImporter,
      '/workspace'
    );
  });

  describe('ensureSeraInstantiated', () => {
    it('returns existing instance if sera is already instantiated', async () => {
      mockRegistry.listInstances = vi.fn().mockResolvedValue([{ name: 'sera', id: 'existing-id' }]);

      const result = await bootstrapService.ensureSeraInstantiated();

      expect(result).toEqual({ bootstrapped: true, seraInstanceId: 'existing-id' });
      expect(mockImporter.importAll).not.toHaveBeenCalled();
    });

    it('bootstraps new instance if sera is not instantiated', async () => {
      mockRegistry.listInstances = vi.fn().mockResolvedValue([]);
      mockRegistry.getTemplate = vi.fn().mockResolvedValue({ name: 'sera' });
      mockRegistry.createInstance = vi.fn().mockResolvedValue({ id: 'new-id' });

      const result = await bootstrapService.ensureSeraInstantiated();

      expect(mockImporter.importAll).toHaveBeenCalled();
      expect(mockRegistry.getTemplate).toHaveBeenCalledWith('sera');
      expect(mockRegistry.createInstance).toHaveBeenCalledWith({
        name: 'sera',
        displayName: 'Sera (Primary Agent)',
        templateRef: 'sera',
        circle: 'default',
        lifecycleMode: 'persistent',
      });
      expect(result).toEqual({ bootstrapped: true, seraInstanceId: 'new-id' });
    });

    it('throws error if template is not found after import', async () => {
      mockRegistry.listInstances = vi.fn().mockResolvedValue([]);
      mockRegistry.getTemplate = vi.fn().mockResolvedValue(null);

      await expect(bootstrapService.ensureSeraInstantiated()).rejects.toThrow(
        'Sera template not found in registry after import. Bootstrap failed.'
      );
      expect(mockImporter.importAll).toHaveBeenCalled();
      expect(mockRegistry.createInstance).not.toHaveBeenCalled();
    });
  });

  describe('getBootstrapStatus', () => {
    it('returns bootstrapped true and seraInstanceId when sera instance exists', async () => {
      mockRegistry.listInstances = vi.fn().mockResolvedValue([
        { name: 'other-agent', id: '123' },
        { name: 'sera', id: 'sera-id' },
      ]);

      const result = await bootstrapService.getBootstrapStatus();

      expect(result).toEqual({ bootstrapped: true, seraInstanceId: 'sera-id' });
    });

    it('returns bootstrapped false and null when sera instance does not exist', async () => {
      mockRegistry.listInstances = vi.fn().mockResolvedValue([
        { name: 'other-agent', id: '123' },
      ]);

      const result = await bootstrapService.getBootstrapStatus();

      expect(result).toEqual({ bootstrapped: false, seraInstanceId: null });
    });

    it('returns bootstrapped false and null when no instances exist', async () => {
      mockRegistry.listInstances = vi.fn().mockResolvedValue([]);

      const result = await bootstrapService.getBootstrapStatus();

      expect(result).toEqual({ bootstrapped: false, seraInstanceId: null });
    });
  });
});
