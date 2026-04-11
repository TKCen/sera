import { describe, it, expect, beforeEach } from 'vitest';
import { LoopGuard } from './LoopGuard.js';

describe('LoopGuard', () => {
  let guard: LoopGuard;

  beforeEach(() => {
    guard = new LoopGuard();
  });

  // ── recordCall ──────────────────────────────────────────────────────────────

  it('should return "ok" for a new call', () => {
    const result = guard.recordCall('web-search', { query: 'hello' });
    expect(result.status).toBe('ok');
    expect(result.count).toBe(1);
  });

  it('should return "ok" for 2 identical calls', () => {
    guard.recordCall('web-search', { query: 'hello' });
    const result = guard.recordCall('web-search', { query: 'hello' });
    expect(result.status).toBe('ok');
    expect(result.count).toBe(2);
  });

  it('should return "warn" at WARN_THRESHOLD (3) identical calls', () => {
    guard.recordCall('web-search', { query: 'hello' });
    guard.recordCall('web-search', { query: 'hello' });
    const result = guard.recordCall('web-search', { query: 'hello' });
    expect(result.status).toBe('warn');
    expect(result.count).toBe(3);
  });

  it('should return "block" at BLOCK_THRESHOLD (5) identical calls', () => {
    for (let i = 0; i < 4; i++) {
      guard.recordCall('web-search', { query: 'hello' });
    }
    const result = guard.recordCall('web-search', { query: 'hello' });
    expect(result.status).toBe('block');
    expect(result.count).toBe(5);
  });

  it('should track different calls independently', () => {
    guard.recordCall('web-search', { query: 'hello' });
    guard.recordCall('web-search', { query: 'hello' });
    guard.recordCall('web-search', { query: 'hello' }); // warn

    // Different args — should be ok
    const result = guard.recordCall('web-search', { query: 'world' });
    expect(result.status).toBe('ok');
    expect(result.count).toBe(1);
  });

  it('should track different tool names independently', () => {
    guard.recordCall('web-search', { query: 'hello' });
    guard.recordCall('web-search', { query: 'hello' });
    guard.recordCall('web-search', { query: 'hello' });

    const result = guard.recordCall('file-read', { query: 'hello' });
    expect(result.status).toBe('ok');
    expect(result.count).toBe(1);
  });

  // ── reset ───────────────────────────────────────────────────────────────────

  it('should clear all state on reset', () => {
    guard.recordCall('web-search', { query: 'hello' });
    guard.recordCall('web-search', { query: 'hello' });
    guard.recordCall('web-search', { query: 'hello' });

    guard.reset();

    const result = guard.recordCall('web-search', { query: 'hello' });
    expect(result.status).toBe('ok');
    expect(result.count).toBe(1);
  });

  it('should report size correctly', () => {
    guard.recordCall('tool-a', { x: 1 });
    guard.recordCall('tool-b', { x: 2 });
    guard.recordCall('tool-a', { x: 1 }); // duplicate, same hash

    expect(guard.size).toBe(2);
  });

  // ── hashCall ────────────────────────────────────────────────────────────────

  it('should produce deterministic hashes', () => {
    const h1 = LoopGuard.hashCall('web-search', { query: 'test' });
    const h2 = LoopGuard.hashCall('web-search', { query: 'test' });
    expect(h1).toBe(h2);
  });

  it('should produce different hashes for different inputs', () => {
    const h1 = LoopGuard.hashCall('web-search', { query: 'test' });
    const h2 = LoopGuard.hashCall('web-search', { query: 'other' });
    expect(h1).not.toBe(h2);
  });

  it('should produce 8-character hex strings', () => {
    const hash = LoopGuard.hashCall('test', {});
    expect(hash).toMatch(/^[0-9a-f]{8}$/);
  });
});
