import { CronExpressionParser } from 'cron-parser';

/**
 * Validates a cron expression.
 * @returns null if valid, or a human-readable error string.
 */
export function validateCronExpression(expr: string): string | null {
  try {
    CronExpressionParser.parse(expr);
    return null;
  } catch (err) {
    return (err as Error).message;
  }
}

/**
 * Computes the next run time from a cron expression.
 * @returns the next Date, or null if the expression is invalid.
 */
export function computeNextRunAt(expr: string): Date | null {
  try {
    const interval = CronExpressionParser.parse(expr);
    return interval.next().toDate();
  } catch {
    return null;
  }
}
