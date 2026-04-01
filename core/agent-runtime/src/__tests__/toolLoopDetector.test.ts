import { describe, it, expect } from 'vitest';
import { ToolLoopDetector, flattenToSet, jaccardSimilarity } from '../toolLoopDetector.js';

describe('ToolLoopDetector', () => {
  describe('consecutive detection', () => {
    it('detects 3 consecutive calls to the same tool', () => {
      const detector = new ToolLoopDetector();
      detector.record('file-read', { path: '/a.txt' });
      detector.record('file-read', { path: '/b.txt' });
      const verdict = detector.record('file-read', { path: '/c.txt' });
      expect(verdict.detected).toBe(true);
      expect(verdict.kind).toBe('consecutive');
    });

    it('does not trigger on 2 consecutive calls', () => {
      const detector = new ToolLoopDetector();
      detector.record('file-read', { path: '/a.txt' });
      const verdict = detector.record('file-read', { path: '/b.txt' });
      expect(verdict.detected).toBe(false);
    });

    it('does not trigger for exempt tools', () => {
      const detector = new ToolLoopDetector({ exemptTools: new Set(['shell-exec']) });
      detector.record('shell-exec', { command: 'ls' });
      detector.record('shell-exec', { command: 'pwd' });
      const verdict = detector.record('shell-exec', { command: 'cat foo' });
      expect(verdict.detected).toBe(false);
    });

    it('respects custom threshold', () => {
      const detector = new ToolLoopDetector({ consecutiveThreshold: 5 });
      for (let i = 0; i < 4; i++) {
        const v = detector.record('file-read', { path: `/file${i}` });
        expect(v.detected).toBe(false);
      }
      const verdict = detector.record('file-read', { path: '/file4' });
      expect(verdict.detected).toBe(true);
      expect(verdict.kind).toBe('consecutive');
    });

    it('resets when a different tool is used', () => {
      const detector = new ToolLoopDetector();
      detector.record('file-read', { path: '/a.txt' });
      detector.record('file-read', { path: '/b.txt' });
      detector.record('shell-exec', { command: 'ls' }); // breaks the streak
      const verdict = detector.record('file-read', { path: '/c.txt' });
      expect(verdict.detected).toBe(false);
    });
  });

  describe('oscillation detection', () => {
    it('detects A-B-A-B-A-B oscillation (3 cycles)', () => {
      const detector = new ToolLoopDetector();
      detector.record('file-read', { path: '/a' });
      detector.record('file-write', { path: '/b', content: 'x' });
      detector.record('file-read', { path: '/a' });
      detector.record('file-write', { path: '/b', content: 'y' });
      detector.record('file-read', { path: '/a' });
      const verdict = detector.record('file-write', { path: '/b', content: 'z' });
      expect(verdict.detected).toBe(true);
      expect(verdict.kind).toBe('oscillation');
    });

    it('does not trigger on A-B-A-B (only 2 cycles)', () => {
      const detector = new ToolLoopDetector();
      detector.record('file-read', { path: '/a' });
      detector.record('file-write', { path: '/b', content: 'x' });
      detector.record('file-read', { path: '/a' });
      const verdict = detector.record('file-write', { path: '/b', content: 'y' });
      expect(verdict.detected).toBe(false);
    });

    it('does not trigger on A-B-C-A-B-C pattern', () => {
      const detector = new ToolLoopDetector();
      detector.record('file-read', {});
      detector.record('file-write', {});
      detector.record('shell-exec', {});
      detector.record('file-read', {});
      detector.record('file-write', {});
      const verdict = detector.record('shell-exec', {});
      expect(verdict.detected).toBe(false);
    });
  });

  describe('similarity detection', () => {
    it('detects near-duplicate arguments (>80% similar)', () => {
      const detector = new ToolLoopDetector();
      // 9 matching out of 10 total key-value pairs → Jaccard = 9/11 = 0.818 > 0.8
      const baseArgs = { path: '/data.txt', limit: 100, mode: 'text', encoding: 'utf8', format: 'raw', verbose: true, recursive: false, depth: 3, sort: 'name' };
      detector.record('file-read', { ...baseArgs, offset: 0 });
      detector.record('file-read', { ...baseArgs, offset: 10 });
      const verdict = detector.record('file-read', { ...baseArgs, offset: 20 });
      expect(verdict.detected).toBe(true);
      expect(verdict.kind).toBe('similarity');
    });

    it('falls back to consecutive when similarity is below threshold', () => {
      const detector = new ToolLoopDetector();
      // 3 calls to same tool with very different args → consecutive triggers, not similarity
      detector.record('file-read', { path: '/file-a.txt' });
      detector.record('file-read', { path: '/file-b.txt' });
      const verdict = detector.record('file-read', { path: '/file-c.txt' });
      // Consecutive detection still triggers (same tool 3x)
      expect(verdict.detected).toBe(true);
      expect(verdict.kind).toBe('consecutive');
    });

    it('does not compare across different tool names', () => {
      const detector = new ToolLoopDetector();
      detector.record('file-read', { path: '/data.txt', offset: 0 });
      detector.record('file-write', { path: '/data.txt', content: 'x' });
      const verdict = detector.record('file-read', { path: '/data.txt', offset: 0 });
      // Only 2 file-read calls (not 3), so neither similarity nor consecutive triggers
      expect(verdict.detected).toBe(false);
    });
  });

  describe('escalation', () => {
    it('shouldForceTextResponse returns false before maxWarnings', () => {
      const detector = new ToolLoopDetector();
      expect(detector.shouldForceTextResponse()).toBe(false);
      detector.acknowledgeWarning();
      expect(detector.shouldForceTextResponse()).toBe(false);
    });

    it('shouldForceTextResponse returns true after maxWarnings', () => {
      const detector = new ToolLoopDetector({ maxWarnings: 2 });
      detector.acknowledgeWarning();
      detector.acknowledgeWarning();
      expect(detector.shouldForceTextResponse()).toBe(true);
    });

    it('reset clears all state', () => {
      const detector = new ToolLoopDetector();
      detector.record('file-read', { path: '/a' });
      detector.record('file-read', { path: '/b' });
      detector.acknowledgeWarning();
      detector.acknowledgeWarning();
      expect(detector.shouldForceTextResponse()).toBe(true);

      detector.reset();
      expect(detector.shouldForceTextResponse()).toBe(false);
      // History is also cleared — no detection possible
      const verdict = detector.record('file-read', { path: '/c' });
      expect(verdict.detected).toBe(false);
    });
  });
});

describe('flattenToSet', () => {
  it('flattens flat object', () => {
    const result = flattenToSet({ a: 1, b: 'hello' });
    expect(result).toEqual(new Set(['a=1', 'b=hello']));
  });

  it('flattens nested object with dot-paths', () => {
    const result = flattenToSet({ a: { b: { c: 42 } } });
    expect(result).toEqual(new Set(['a.b.c=42']));
  });

  it('returns empty set for empty object', () => {
    const result = flattenToSet({});
    expect(result.size).toBe(0);
  });

  it('handles arrays as values', () => {
    const result = flattenToSet({ arr: [1, 2, 3] });
    expect(result).toEqual(new Set(['arr=1,2,3']));
  });

  it('handles null values', () => {
    const result = flattenToSet({ key: null });
    expect(result).toEqual(new Set(['key=null']));
  });
});

describe('jaccardSimilarity', () => {
  it('returns 1.0 for identical sets', () => {
    const a = new Set(['x=1', 'y=2']);
    expect(jaccardSimilarity(a, a)).toBe(1.0);
  });

  it('returns 0.0 for completely disjoint sets', () => {
    const a = new Set(['x=1']);
    const b = new Set(['y=2']);
    expect(jaccardSimilarity(a, b)).toBe(0.0);
  });

  it('returns 1.0 for two empty sets', () => {
    expect(jaccardSimilarity(new Set(), new Set())).toBe(1.0);
  });

  it('returns correct partial overlap', () => {
    const a = new Set(['x=1', 'y=2', 'z=3']);
    const b = new Set(['x=1', 'y=2', 'w=4']);
    // intersection = 2, union = 4
    expect(jaccardSimilarity(a, b)).toBe(0.5);
  });
});
