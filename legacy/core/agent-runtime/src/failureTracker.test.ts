import { describe, it, expect, beforeEach } from 'vitest';
import { FailureTracker } from './failureTracker.js';

describe('FailureTracker', () => {
  let tracker: FailureTracker;

  beforeEach(() => {
    tracker = new FailureTracker();
  });

  describe('recordFailure', () => {
    it('increments count on each failure', () => {
      expect(tracker.recordFailure('shell-exec')).toBe(1);
      expect(tracker.recordFailure('shell-exec')).toBe(2);
      expect(tracker.recordFailure('shell-exec')).toBe(3);
    });

    it('tracks multiple tools independently', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('file-read');
      expect(tracker.recordFailure('shell-exec')).toBe(3);
      expect(tracker.recordFailure('file-read')).toBe(2);
    });
  });

  describe('recordSuccess', () => {
    it('resets failure count for a tool', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      tracker.recordSuccess('shell-exec');
      expect(tracker.recordFailure('shell-exec')).toBe(1);
    });

    it('does not affect other tools', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('file-read');
      tracker.recordSuccess('shell-exec');
      expect(tracker.recordFailure('file-read')).toBe(2);
    });

    it('is a no-op for tools with no failures', () => {
      tracker.recordSuccess('shell-exec');
      expect(tracker.hasThresholdExceeded()).toBe(false);
    });
  });

  describe('hasThresholdExceeded', () => {
    it('returns false when no tool has failed enough', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      expect(tracker.hasThresholdExceeded()).toBe(false);
    });

    it('returns true when a tool reaches the default threshold of 3', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      expect(tracker.hasThresholdExceeded()).toBe(true);
    });

    it('returns false after a successful reset clears the offending tool', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      tracker.recordSuccess('shell-exec');
      expect(tracker.hasThresholdExceeded()).toBe(false);
    });

    it('respects custom threshold', () => {
      const t = new FailureTracker(5);
      for (let i = 0; i < 4; i++) t.recordFailure('tool');
      expect(t.hasThresholdExceeded()).toBe(false);
      t.recordFailure('tool');
      expect(t.hasThresholdExceeded()).toBe(true);
    });
  });

  describe('getTopFailures', () => {
    it('returns empty array when no tool hits threshold', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      expect(tracker.getTopFailures()).toEqual([]);
    });

    it('returns tools that hit threshold sorted by count descending', () => {
      // tool-a: 5 failures, tool-b: 3 failures
      for (let i = 0; i < 5; i++) tracker.recordFailure('tool-a');
      for (let i = 0; i < 3; i++) tracker.recordFailure('tool-b');
      const top = tracker.getTopFailures();
      expect(top[0]).toEqual({ toolName: 'tool-a', count: 5 });
      expect(top[1]).toEqual({ toolName: 'tool-b', count: 3 });
    });

    it('caps results at topN (default 3)', () => {
      for (const name of ['a', 'b', 'c', 'd']) {
        for (let i = 0; i < 3; i++) tracker.recordFailure(name);
      }
      expect(tracker.getTopFailures()).toHaveLength(3);
    });

    it('respects custom topN', () => {
      const t = new FailureTracker(3, 2);
      for (const name of ['a', 'b', 'c']) {
        for (let i = 0; i < 3; i++) t.recordFailure(name);
      }
      expect(t.getTopFailures()).toHaveLength(2);
    });
  });

  describe('buildContextString', () => {
    it('returns null when no tool has reached threshold', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      expect(tracker.buildContextString()).toBeNull();
    });

    it('returns a warning string when threshold is exceeded', () => {
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      tracker.recordFailure('shell-exec');
      const ctx = tracker.buildContextString();
      expect(ctx).not.toBeNull();
      expect(ctx).toContain('shell-exec');
      expect(ctx).toContain('3');
      expect(ctx).toContain('Consider an alternative approach');
    });

    it('includes the warning prefix', () => {
      for (let i = 0; i < 3; i++) tracker.recordFailure('file-read');
      const ctx = tracker.buildContextString();
      expect(ctx).toMatch(/^⚠️ Recent issues:/);
    });

    it('includes multiple failing tools', () => {
      for (let i = 0; i < 3; i++) tracker.recordFailure('tool-a');
      for (let i = 0; i < 4; i++) tracker.recordFailure('tool-b');
      const ctx = tracker.buildContextString();
      expect(ctx).toContain('tool-a');
      expect(ctx).toContain('tool-b');
    });

    it('caps at topN tools in the context string', () => {
      const t = new FailureTracker(3, 2);
      for (const name of ['a', 'b', 'c']) {
        for (let i = 0; i < 3; i++) t.recordFailure(name);
      }
      const ctx = t.buildContextString();
      // Only 2 tools should appear; 'c' should be omitted since all have equal count
      // and only top 2 are kept
      const matches = ctx!.match(/has failed/g);
      expect(matches).toHaveLength(2);
    });

    it('returns null after success resets the only failing tool below threshold', () => {
      for (let i = 0; i < 3; i++) tracker.recordFailure('shell-exec');
      tracker.recordSuccess('shell-exec');
      expect(tracker.buildContextString()).toBeNull();
    });
  });
});
