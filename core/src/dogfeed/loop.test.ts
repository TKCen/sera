import { describe, it, expect, vi, beforeEach } from 'vitest';
import { DogfeedLoop } from './loop.js';

// Mock dependencies — use factory functions so vi.clearAllMocks doesn't remove implementations
vi.mock('./analyzer.js', () => {
  return {
    DogfeedAnalyzer: class {
      pickNext = vi.fn().mockReturnValue(undefined);
      scanTaskFile = vi.fn().mockReturnValue([]);
      markDone = vi.fn();
    },
  };
});

vi.mock('./agent-spawner.js', () => {
  return {
    AgentSpawner: class {
      routeTask = vi.fn().mockReturnValue('pi-agent');
      spawn = vi.fn();
    },
  };
});

vi.mock('./verify-merge.js', () => {
  return {
    VerifyMerge: class {
      createBranch = vi.fn();
      commitChanges = vi.fn().mockResolvedValue({ filesChanged: 0, insertions: 0, deletions: 0 });
      runCI = vi.fn().mockResolvedValue({ passed: true, output: '', durationMs: 0 });
      mergeToMain = vi.fn();
      cleanup = vi.fn();
      pushBranch = vi.fn();
    },
  };
});

vi.mock('node:fs', () => ({
  default: {
    existsSync: vi.fn().mockReturnValue(false),
    readFileSync: vi.fn().mockReturnValue(''),
    writeFileSync: vi.fn(),
  },
}));

describe('DogfeedLoop', () => {
  let loop: DogfeedLoop;

  beforeEach(() => {
    loop = new DogfeedLoop({ repoRoot: '/tmp/test', pushToRemote: false, autoMerge: false });
  });

  describe('getStatus', () => {
    it('returns idle status initially', () => {
      const status = loop.getStatus();
      expect(status.phase).toBe('idle');
    });
  });

  describe('getLastResult', () => {
    it('returns undefined before any cycle', () => {
      expect(loop.getLastResult()).toBeUndefined();
    });
  });

  describe('runCycle', () => {
    it('returns error result when no tasks available', async () => {
      const result = await loop.runCycle();
      expect(result.success).toBe(false);
      expect(result.error).toBe('No ready tasks available');
    });
  });
});
