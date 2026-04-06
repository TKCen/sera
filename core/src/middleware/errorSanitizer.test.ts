import { describe, it, expect } from 'vitest';
import { sanitizeErrorMessage } from './errorSanitizer.js';

describe('sanitizeErrorMessage', () => {
  it('passes through safe user-facing messages', () => {
    expect(sanitizeErrorMessage('Block persona not found')).toBe('Block persona not found');
    expect(sanitizeErrorMessage('Agent not found')).toBe('Agent not found');
    expect(sanitizeErrorMessage('Invalid request body')).toBe('Invalid request body');
    expect(sanitizeErrorMessage('Not found')).toBe('Not found');
  });

  it('blocks messages with file paths', () => {
    expect(sanitizeErrorMessage('Error at /app/src/routes/chat.ts:45')).toBe(
      'An internal error occurred'
    );
    expect(sanitizeErrorMessage('ENOENT: /home/user/.config')).toBe('An internal error occurred');
  });

  it('blocks messages with SQL fragments', () => {
    expect(sanitizeErrorMessage('SELECT * FROM agent_instances WHERE id = 123')).toBe(
      'An internal error occurred'
    );
    expect(sanitizeErrorMessage('INSERT INTO token_usage failed')).toBe(
      'An internal error occurred'
    );
  });

  it('blocks messages with stack traces', () => {
    expect(sanitizeErrorMessage('TypeError: at Object.handler (/app/src/index.ts:100)')).toBe(
      'An internal error occurred'
    );
  });

  it('blocks messages with connection errors', () => {
    expect(sanitizeErrorMessage('connect ECONNREFUSED 127.0.0.1:5432')).toBe(
      'An internal error occurred'
    );
  });

  it('blocks messages with credential references', () => {
    expect(sanitizeErrorMessage('pg_advisory_lock failed')).toBe('An internal error occurred');
  });

  it('passes through short non-sensitive messages', () => {
    expect(sanitizeErrorMessage('Model not available')).toBe('Model not available');
    expect(sanitizeErrorMessage('Request timeout')).toBe('Request timeout');
  });

  it('blocks very long messages (likely stack traces)', () => {
    const longMsg = 'a'.repeat(250);
    expect(sanitizeErrorMessage(longMsg)).toBe('An internal error occurred');
  });
});
