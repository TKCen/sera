import { describe, it, expect } from 'vitest';
import {
  utilPct,
  budgetBarColor,
  cn,
  formatDistanceToNow,
  formatTime,
  formatElapsed,
} from './utils';

describe('utils', () => {
  describe('utilPct', () => {
    it('returns 0 when limit is undefined or 0', () => {
      expect(utilPct(50)).toBe(0);
      expect(utilPct(50, 0)).toBe(0);
      expect(utilPct(50, -10)).toBe(0);
    });

    it('calculates percentage correctly', () => {
      expect(utilPct(50, 100)).toBe(50);
      expect(utilPct(25, 100)).toBe(25);
      expect(utilPct(1, 10)).toBe(10);
      expect(utilPct(2, 4)).toBe(50);
    });

    it('caps percentage at 100', () => {
      expect(utilPct(150, 100)).toBe(100);
      expect(utilPct(20, 10)).toBe(100);
    });

    it('handles 0 current properly', () => {
      expect(utilPct(0, 100)).toBe(0);
    });
  });

  describe('budgetBarColor', () => {
    it('returns success color for < 70%', () => {
      expect(budgetBarColor(0)).toBe('bg-sera-success');
      expect(budgetBarColor(50)).toBe('bg-sera-success');
      expect(budgetBarColor(69)).toBe('bg-sera-success');
    });

    it('returns warning color for >= 70% and < 90%', () => {
      expect(budgetBarColor(70)).toBe('bg-sera-warning');
      expect(budgetBarColor(80)).toBe('bg-sera-warning');
      expect(budgetBarColor(89)).toBe('bg-sera-warning');
    });

    it('returns error color for >= 90%', () => {
      expect(budgetBarColor(90)).toBe('bg-sera-error');
      expect(budgetBarColor(95)).toBe('bg-sera-error');
      expect(budgetBarColor(100)).toBe('bg-sera-error');
      expect(budgetBarColor(150)).toBe('bg-sera-error');
    });
  });

  describe('cn', () => {
    it('merges class names properly', () => {
      expect(cn('bg-red-500', 'text-white')).toBe('bg-red-500 text-white');
    });

    it('resolves tailwind conflicts', () => {
      expect(cn('p-4', 'p-8')).toBe('p-8');
      expect(cn('bg-red-500', 'bg-blue-500')).toBe('bg-blue-500');
    });

    it('handles conditional classes', () => {
      expect(cn('p-4', undefined, null, 'text-red-500')).toBe('p-4 text-red-500');
    });
  });

  describe('formatTime', () => {
    it('formats a valid ISO timestamp as HH:MM:SS', () => {
      // Use a fixed date to avoid locale issues
      const result = formatTime('2026-01-15T14:30:45Z');
      // Should contain digits and colons (locale-dependent format)
      expect(result).toMatch(/\d{1,2}:\d{2}:\d{2}/);
    });

    it('returns empty string for invalid timestamp', () => {
      expect(formatTime('not-a-date')).toBe('');
      expect(formatTime('')).toBe('');
    });
  });

  describe('formatElapsed', () => {
    it('formats milliseconds', () => {
      expect(formatElapsed('2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.500Z')).toBe('500ms');
    });

    it('formats seconds', () => {
      expect(formatElapsed('2026-01-01T00:00:00Z', '2026-01-01T00:00:05Z')).toBe('5.0s');
    });

    it('formats minutes and seconds', () => {
      expect(formatElapsed('2026-01-01T00:00:00Z', '2026-01-01T00:02:30Z')).toBe('2m 30s');
    });

    it('returns empty string for invalid or negative elapsed', () => {
      expect(formatElapsed('not-a-date', '2026-01-01T00:00:00Z')).toBe('');
      expect(formatElapsed('2026-01-01T00:00:05Z', '2026-01-01T00:00:00Z')).toBe('');
    });
  });

  describe('formatDistanceToNow', () => {
    it('formats less than a minute ago correctly', () => {
      const now = Date.now();
      const past = new Date(now - 30_000).toISOString();
      const future = new Date(now + 30_000).toISOString();

      expect(formatDistanceToNow(past)).toBe('just now');
      expect(formatDistanceToNow(future)).toBe('in a moment');
    });

    it('formats minutes correctly', () => {
      const now = Date.now();
      const past = new Date(now - 5 * 60_000).toISOString();
      const future = new Date(now + 15 * 60_000).toISOString();

      expect(formatDistanceToNow(past)).toBe('5m ago');
      expect(formatDistanceToNow(future)).toBe('in 15m');
    });

    it('formats hours correctly', () => {
      const now = Date.now();
      const past = new Date(now - 3 * 3_600_000).toISOString();
      const future = new Date(now + 5 * 3_600_000).toISOString();

      expect(formatDistanceToNow(past)).toBe('3h ago');
      expect(formatDistanceToNow(future)).toBe('in 5h');
    });

    it('formats days correctly', () => {
      const now = Date.now();
      const past = new Date(now - 2 * 86_400_000).toISOString();
      const future = new Date(now + 4 * 86_400_000).toISOString();

      expect(formatDistanceToNow(past)).toBe('2d ago');
      expect(formatDistanceToNow(future)).toBe('in 4d');
    });
  });
});
