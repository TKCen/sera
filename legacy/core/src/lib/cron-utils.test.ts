import { describe, it, expect } from 'vitest';
import { validateCronExpression, computeNextRunAt } from './cron-utils.js';

describe('cron-utils', () => {
  describe('validateCronExpression', () => {
    it('returns null for valid 5-field cron expression', () => {
      expect(validateCronExpression('0 */8 * * *')).toBeNull();
      expect(validateCronExpression('*/5 * * * *')).toBeNull();
      expect(validateCronExpression('0 6,18 * * *')).toBeNull();
      expect(validateCronExpression('0 0 * * 0')).toBeNull();
    });

    it('returns null for standard minute-level expressions', () => {
      expect(validateCronExpression('* * * * *')).toBeNull();
      expect(validateCronExpression('0 0 1 1 *')).toBeNull();
    });

    it('returns error string for invalid expression', () => {
      const result = validateCronExpression('not-a-cron');
      expect(result).toBeTypeOf('string');
      expect(result!.length).toBeGreaterThan(0);
    });

    it('returns error for too few fields', () => {
      const result = validateCronExpression('* *');
      expect(result).toBeTypeOf('string');
    });

    it('returns error for out-of-range values', () => {
      const result = validateCronExpression('99 99 99 99 99');
      expect(result).toBeTypeOf('string');
    });
  });

  describe('computeNextRunAt', () => {
    it('returns a Date for valid expression', () => {
      const next = computeNextRunAt('* * * * *');
      expect(next).toBeInstanceOf(Date);
      expect(next!.getTime()).toBeGreaterThan(Date.now());
    });

    it('returns a future date', () => {
      const next = computeNextRunAt('0 0 * * *');
      expect(next).toBeInstanceOf(Date);
      expect(next!.getTime()).toBeGreaterThan(Date.now());
    });

    it('returns null for invalid expression', () => {
      expect(computeNextRunAt('invalid')).toBeNull();
    });
  });
});
