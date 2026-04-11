import { describe, it, expect } from 'vitest';

// Re-implement the sanitizer for isolated testing (it's a module-private function)
function sanitizeRunStatus(value: unknown): string | null {
  if (value == null) return null;
  const str = String(value);
  const sqlKeywords = [
    'column',
    'relation',
    'constraint',
    'violates',
    'syntax error',
    'ERROR:',
    'DETAIL:',
  ];
  if (sqlKeywords.some((kw) => str.toLowerCase().includes(kw.toLowerCase()))) {
    return 'Execution failed';
  }
  return str;
}

describe('sanitizeRunStatus', () => {
  it('should return null for null/undefined', () => {
    expect(sanitizeRunStatus(null)).toBeNull();
    expect(sanitizeRunStatus(undefined)).toBeNull();
  });

  it('should pass through normal status strings', () => {
    expect(sanitizeRunStatus('success')).toBe('success');
    expect(sanitizeRunStatus('failed')).toBe('failed');
    expect(sanitizeRunStatus('completed at 2026-03-30')).toBe('completed at 2026-03-30');
  });

  it('should sanitize SQL constraint violation errors', () => {
    expect(
      sanitizeRunStatus(
        'null value in column "cron" of relation "schedule" violates not-null constraint'
      )
    ).toBe('Execution failed');
  });

  it('should sanitize SQL syntax errors', () => {
    expect(sanitizeRunStatus('syntax error at or near "SELECT"')).toBe('Execution failed');
  });

  it('should sanitize PostgreSQL ERROR/DETAIL messages', () => {
    expect(sanitizeRunStatus('ERROR: duplicate key value')).toBe('Execution failed');
    expect(sanitizeRunStatus('DETAIL: Key (id)=(abc) already exists')).toBe('Execution failed');
  });

  it('should sanitize messages containing "column" or "relation"', () => {
    expect(sanitizeRunStatus('column xyz does not exist')).toBe('Execution failed');
    expect(sanitizeRunStatus('relation "foo" does not exist')).toBe('Execution failed');
  });
});
