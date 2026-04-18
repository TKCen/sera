import { describe, it, expect, vi, beforeEach } from 'vitest';
import { VerifyMerge } from './verify-merge.js';
import { createDefaultConfig } from './constants.js';

// Mock simple-git
vi.mock('simple-git', () => ({
  simpleGit: vi.fn(
    () =>
      ({
        checkoutLocalBranch: vi.fn(),
        checkout: vi.fn(),
        push: vi.fn(),
        fetch: vi.fn(),
        diff: vi.fn().mockResolvedValue(''),
        merge: vi.fn(),
        stash: vi.fn().mockResolvedValue('No local changes to save'),
        revert: vi.fn(),
        reset: vi.fn(),
        deleteLocalBranch: vi.fn(),
      }) as unknown
  ),
}));

describe('VerifyMerge', () => {
  let verifier: VerifyMerge;

  beforeEach(() => {
    const config = createDefaultConfig({
      repoRoot: '/tmp/test-repo',
      pushToRemote: false,
      autoMerge: false,
    });
    verifier = new VerifyMerge(config);
  });

  describe('createBranch', () => {
    it('creates a branch and pushes it', async () => {
      await verifier.createBranch('dogfeed/1-test');
      // Should not throw — git operations are mocked
    });
  });

  describe('pullBranch', () => {
    it('returns zero changes for empty diff', async () => {
      const stats = await verifier.pullBranch('dogfeed/1-test');
      expect(stats.filesChanged).toBe(0);
      expect(stats.insertions).toBe(0);
      expect(stats.deletions).toBe(0);
    });
  });

  describe('cleanup', () => {
    it('cleans up branch without throwing', async () => {
      await verifier.cleanup('dogfeed/1-test');
    });
  });

  describe('mergeToMain', () => {
    it('merges without throwing', async () => {
      await verifier.mergeToMain('dogfeed/1-test');
    });
  });

  describe('revertLastMerge', () => {
    it('reverts without throwing', async () => {
      await verifier.revertLastMerge();
    });
  });
});
