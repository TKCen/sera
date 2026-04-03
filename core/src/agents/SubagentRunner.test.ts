import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SubagentRunner } from './SubagentRunner.js';
import { SandboxManager } from '../sandbox/SandboxManager.js';
import { TierPolicy, PolicyViolationError } from '../sandbox/TierPolicy.js';
import type { AgentManifest } from './manifest/types.js';

// Mock SandboxManager
vi.mock('../sandbox/SandboxManager.js', () => {
  class Mock {
    countSubagents = vi.fn();
    spawn = vi.fn();
    listContainers = vi.fn();
    getLogs = vi.fn();
    remove = vi.fn();
  }
  return { SandboxManager: Mock };
});

// Mock TierPolicy
vi.mock('../sandbox/TierPolicy.js', () => {
  return {
    TierPolicy: {
      checkInstanceLimit: vi.fn(),
    },
    PolicyViolationError: class extends Error {
      constructor(message: string, public agentName: string, public violation: string) {
        super(message);
        this.name = 'PolicyViolationError';
      }
    },
  };
});

// Mock Logger
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

describe('SubagentRunner', () => {
  let runner: SubagentRunner;
  let sandboxManager: SandboxManager;

  const mockParentManifest: AgentManifest = {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: { name: 'parent', displayName: 'Parent', tier: 2 },
    identity: { role: 'parent', description: '' },
    model: { provider: 'openai', name: 'gpt-4' },
    subagents: {
      allowed: [{ role: 'researcher', maxInstances: 2 }],
    },
  } as any;

  beforeEach(() => {
    sandboxManager = new SandboxManager({} as any);
    runner = new SubagentRunner(sandboxManager);
    vi.clearAllMocks();
  });

  describe('spawnSubagent', () => {
    it('should spawn a subagent if allowed', async () => {
      vi.mocked(sandboxManager.countSubagents).mockReturnValue(0);
      vi.mocked(sandboxManager.spawn).mockResolvedValue({
        containerId: 'sub-123',
        status: 'running',
      } as any);

      const sandbox = await runner.spawnSubagent(mockParentManifest, 'researcher', 'search for x');

      expect(sandbox.containerId).toBe('sub-123');
      expect(sandboxManager.spawn).toHaveBeenCalledWith(
        mockParentManifest,
        expect.objectContaining({
          type: 'subagent',
          subagentRole: 'researcher',
          task: 'search for x',
        })
      );
    });

    it('should throw PolicyViolationError if role not allowed', async () => {
      await expect(
        runner.spawnSubagent(mockParentManifest, 'hacker', 'break in')
      ).rejects.toThrow(PolicyViolationError);
    });

    it('should check instance limit via TierPolicy', async () => {
      vi.mocked(sandboxManager.countSubagents).mockReturnValue(2);
      vi.mocked(TierPolicy.checkInstanceLimit).mockImplementation(() => {
        throw new PolicyViolationError('Limit exceeded', 'parent', 'max_instances_exceeded');
      });

      await expect(
        runner.spawnSubagent(mockParentManifest, 'researcher', 'more work')
      ).rejects.toThrow(PolicyViolationError);

      expect(TierPolicy.checkInstanceLimit).toHaveBeenCalledWith(
        mockParentManifest,
        'researcher',
        2
      );
    });
  });

  describe('waitForSubagent', () => {
    it('should return completed status when container stops', async () => {
      const containerId = 'sub-123';
      vi.mocked(sandboxManager.listContainers).mockReturnValue([
        { containerId, status: 'running', subagentRole: 'researcher' } as any,
      ]);

      // Mock polling: first running, then stopped
      vi.mocked(sandboxManager.listContainers)
        .mockReturnValueOnce([{ containerId, status: 'running', subagentRole: 'researcher' } as any])
        .mockReturnValueOnce([{ containerId, status: 'stopped', subagentRole: 'researcher' } as any]);

      vi.mocked(sandboxManager.getLogs).mockResolvedValue('output text');

      const resultPromise = runner.waitForSubagent(mockParentManifest, containerId, 5000);

      const result = await resultPromise;

      expect(result.status).toBe('completed');
      expect(result.output).toBe('output text');
      expect(sandboxManager.remove).toHaveBeenCalledWith(mockParentManifest, containerId);
    });

    it('should return timeout status if subagent takes too long', async () => {
      const containerId = 'sub-123';
      vi.mocked(sandboxManager.listContainers).mockReturnValue([
        { containerId, status: 'running', subagentRole: 'researcher' } as any,
      ]);

      const result = await runner.waitForSubagent(mockParentManifest, containerId, 10);

      expect(result.status).toBe('timeout');
      expect(sandboxManager.remove).toHaveBeenCalledWith(mockParentManifest, containerId);
    });

    it('should handle missing container', async () => {
      vi.mocked(sandboxManager.listContainers).mockReturnValue([]);

      const result = await runner.waitForSubagent(mockParentManifest, 'missing', 1000);

      expect(result.status).toBe('failed');
      expect(result.error).toBe('Container not found');
    });
  });

  describe('getActiveSubagents', () => {
    it('should filter active subagents for parent', () => {
      vi.mocked(sandboxManager.listContainers).mockReturnValue([
        { containerId: '1', type: 'subagent', parentAgent: 'parent' } as any,
        { containerId: '2', type: 'subagent', parentAgent: 'other' } as any,
        { containerId: '3', type: 'agent' } as any,
      ]);

      const active = runner.getActiveSubagents('parent');
      expect(active).toHaveLength(1);
      expect(active[0]!.containerId).toBe('1');
    });
  });
});
