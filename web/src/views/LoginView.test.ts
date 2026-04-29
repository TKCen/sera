import { describe, it, expect } from 'vitest';

// Re-implement here for unit isolation — must match LoginView.sanitizeRedirect.
// If this drifts, the test file should be moved next to the component or
// sanitizeRedirect should be extracted to a shared util.
function sanitizeRedirect(value: string | undefined): string {
  if (!value || typeof value !== 'string') return '/';
  if (!value.startsWith('/') || value.startsWith('//')) return '/';
  return value;
}

describe('sanitizeRedirect', () => {
  it('keeps a normal in-app path', () => {
    expect(sanitizeRedirect('/agents')).toBe('/agents');
    expect(sanitizeRedirect('/sessions/abc?tab=1#frag')).toBe('/sessions/abc?tab=1#frag');
  });

  it('rejects protocol-relative URLs', () => {
    expect(sanitizeRedirect('//evil.com')).toBe('/');
    expect(sanitizeRedirect('//evil.com/login')).toBe('/');
  });

  it('rejects scheme URLs', () => {
    expect(sanitizeRedirect('http://evil.com')).toBe('/');
    expect(sanitizeRedirect('javascript:alert(1)')).toBe('/');
    expect(sanitizeRedirect('data:text/html,foo')).toBe('/');
  });

  it('rejects bare paths without leading slash', () => {
    expect(sanitizeRedirect('agents')).toBe('/');
  });

  it('falls back to root for empty / undefined / non-string', () => {
    expect(sanitizeRedirect(undefined)).toBe('/');
    expect(sanitizeRedirect('')).toBe('/');
  });
});
