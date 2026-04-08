import { describe, it, expect, vi, beforeEach } from 'vitest';
import { validateProviderBaseUrl, validateProviderBaseUrlAsync } from './url-validation.js';
import dns from 'node:dns/promises';

vi.mock('node:dns/promises', () => ({
  default: {
    lookup: vi.fn(),
  },
}));

describe('url-validation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    delete process.env.SERA_PROVIDER_URL_ALLOWLIST;
  });

  describe('validateProviderBaseUrl (sync)', () => {
    it('accepts empty url', () => {
      expect(validateProviderBaseUrl('')).toEqual({ valid: true });
    });

    it('rejects invalid url', () => {
      expect(validateProviderBaseUrl('not-a-url')).toMatchObject({ valid: false });
    });

    it('rejects non-http/https', () => {
      expect(validateProviderBaseUrl('ftp://example.com')).toMatchObject({
        valid: false,
        reason: expect.stringContaining('must use HTTP or HTTPS'),
      });
    });

    it('rejects http for non-local providers', () => {
      expect(validateProviderBaseUrl('http://example.com')).toMatchObject({
        valid: false,
        reason: expect.stringContaining('must use HTTPS for non-local endpoints'),
      });
    });

    it('accepts http for localhost (local provider)', () => {
      expect(validateProviderBaseUrl('http://localhost', 'lmstudio')).toEqual({ valid: true });
    });

    it('rejects localhost for non-local providers', () => {
      expect(validateProviderBaseUrl('https://localhost', 'openai')).toMatchObject({
        valid: false,
        reason: expect.stringContaining(
          'Localhost endpoints are only permitted for local providers'
        ),
      });
    });

    it('rejects unofficial origins for cloud providers', () => {
      expect(validateProviderBaseUrl('https://attacker.com', 'openai')).toMatchObject({
        valid: false,
        reason: expect.stringContaining('restricted to official origins'),
      });
    });

    it('accepts official origins for cloud providers', () => {
      expect(validateProviderBaseUrl('https://api.openai.com', 'openai')).toEqual({ valid: true });
      expect(validateProviderBaseUrl('https://api.anthropic.com', 'anthropic')).toEqual({
        valid: true,
      });
    });

    it('respects SERA_PROVIDER_URL_ALLOWLIST', () => {
      process.env.SERA_PROVIDER_URL_ALLOWLIST = 'my-internal-proxy.com,attacker.com';
      expect(validateProviderBaseUrl('https://attacker.com', 'openai')).toEqual({ valid: true });
      expect(validateProviderBaseUrl('http://my-internal-proxy.com')).toEqual({ valid: true });
    });

    it('rejects private IP literals', () => {
      expect(validateProviderBaseUrl('https://192.168.1.1')).toMatchObject({
        valid: false,
        reason: expect.stringContaining('private/internal IP address'),
      });
    });
  });

  describe('validateProviderBaseUrlAsync', () => {
    it('rejects host resolving to private IP', async () => {
      vi.mocked(dns.lookup).mockResolvedValue([{ address: '10.0.0.1', family: 4 }] as any);
      const result = await validateProviderBaseUrlAsync('https://some-internal-host.com');
      expect(result).toMatchObject({
        valid: false,
        reason: expect.stringContaining('resolves to a private/internal IP address'),
      });
    });

    it('accepts host resolving to public IP', async () => {
      vi.mocked(dns.lookup).mockResolvedValue([{ address: '8.8.8.8', family: 4 }] as any);
      const result = await validateProviderBaseUrlAsync('https://google.com');
      expect(result).toEqual({ valid: true });
    });

    it('blocks if DNS resolution fails', async () => {
      vi.mocked(dns.lookup).mockRejectedValue(new Error('NXDOMAIN'));
      const result = await validateProviderBaseUrlAsync('https://non-existent.example.com');
      expect(result).toMatchObject({
        valid: false,
        reason: expect.stringContaining('DNS resolution failed'),
      });
    });
  });
});
