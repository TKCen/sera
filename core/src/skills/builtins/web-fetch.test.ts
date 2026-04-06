import { describe, it, expect, vi, beforeEach } from 'vitest';
import { isPrivateIp, resolveAndValidateUrl } from './ssrf.js';
import { webFetchSkill } from './web-fetch.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';

// ---------------------------------------------------------------------------
// isPrivateIp — unit tests
// ---------------------------------------------------------------------------

describe('isPrivateIp', () => {
  // Private IPv4 ranges that must be blocked
  const blockedIpv4 = [
    '127.0.0.1',
    '127.255.255.255',
    '10.0.0.1',
    '10.255.255.255',
    '172.16.0.1',
    '172.31.255.255',
    '192.168.0.1',
    '192.168.255.255',
    '169.254.169.254', // AWS/GCP/Azure metadata
    '169.254.0.1',
    '100.64.0.1', // CGN
    '100.127.255.255',
    '0.0.0.1',
  ];

  // Public IPv4 addresses that must be allowed
  const allowedIpv4 = [
    '1.1.1.1',
    '8.8.8.8',
    '93.184.216.34', // example.com
    '104.26.10.78',
  ];

  // Private/reserved IPv6 addresses that must be blocked
  const blockedIpv6 = [
    '::1',
    '::',
    'fe80::1',
    'fe80::dead:beef',
    'fc00::1',
    'fd00::1',
    'fd12:3456:789a::1',
    '::ffff:127.0.0.1',
    '::ffff:192.168.1.1',
    '64:ff9b::1.2.3.4',
  ];

  // Public IPv6 addresses that must be allowed
  const allowedIpv6 = [
    '2606:4700:4700::1111', // Cloudflare DNS
    '2001:4860:4860::8888', // Google DNS
  ];

  it.each(blockedIpv4)('blocks private IPv4: %s', (ip) => {
    expect(isPrivateIp(ip)).toBe(true);
  });

  it.each(allowedIpv4)('allows public IPv4: %s', (ip) => {
    expect(isPrivateIp(ip)).toBe(false);
  });

  it.each(blockedIpv6)('blocks reserved IPv6: %s', (ip) => {
    expect(isPrivateIp(ip)).toBe(true);
  });

  it.each(allowedIpv6)('allows public IPv6: %s', (ip) => {
    expect(isPrivateIp(ip)).toBe(false);
  });

  it('blocks IPv6-mapped private IPv4 addresses', () => {
    expect(isPrivateIp('::ffff:10.0.0.1')).toBe(true);
    expect(isPrivateIp('::ffff:172.16.0.1')).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// resolveAndValidateUrl — unit tests (DNS mocked)
// ---------------------------------------------------------------------------

vi.mock('node:dns/promises', () => ({
  default: {
    lookup: vi.fn(),
  },
}));

import dns from 'node:dns/promises';

const mockLookup = vi.mocked(dns.lookup);

describe('resolveAndValidateUrl', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('rejects non-http/https schemes', async () => {
    await expect(resolveAndValidateUrl('file:///etc/passwd')).rejects.toThrow(
      'Only http and https URLs are allowed'
    );
    await expect(resolveAndValidateUrl('ftp://example.com')).rejects.toThrow(
      'Only http and https URLs are allowed'
    );
  });

  it('rejects malformed URLs', async () => {
    await expect(resolveAndValidateUrl('not a url')).rejects.toThrow('Invalid URL');
  });

  it('rejects bare private IP literals without DNS lookup', async () => {
    await expect(resolveAndValidateUrl('http://192.168.1.1/secret')).rejects.toThrow(
      'private/reserved IP addresses are not allowed'
    );
    await expect(resolveAndValidateUrl('http://169.254.169.254/latest/meta-data')).rejects.toThrow(
      'private/reserved IP addresses are not allowed'
    );
    await expect(resolveAndValidateUrl('http://10.0.0.1/')).rejects.toThrow(
      'private/reserved IP addresses are not allowed'
    );
    // DNS lookup should NOT have been called for bare IPs
    expect(mockLookup).not.toHaveBeenCalled();
  });

  it('rejects localhost by IP', async () => {
    await expect(resolveAndValidateUrl('http://127.0.0.1/')).rejects.toThrow(
      'private/reserved IP addresses are not allowed'
    );
    expect(mockLookup).not.toHaveBeenCalled();
  });

  it('allows a public IP literal without DNS lookup', async () => {
    const url = await resolveAndValidateUrl('https://1.1.1.1/');
    expect(url.hostname).toBe('1.1.1.1');
    expect(mockLookup).not.toHaveBeenCalled();
  });

  it('resolves hostname and allows public addresses', async () => {
    mockLookup.mockResolvedValueOnce([{ address: '93.184.216.34', family: 4 }] as never);
    const url = await resolveAndValidateUrl('https://example.com/');
    expect(url.hostname).toBe('example.com');
    expect(mockLookup).toHaveBeenCalledWith('example.com', { all: true, family: 0 });
  });

  it('blocks hostname that resolves to a private IP (DNS rebinding)', async () => {
    mockLookup.mockResolvedValueOnce([{ address: '169.254.169.254', family: 4 }] as never);
    await expect(resolveAndValidateUrl('https://evil.example.com/')).rejects.toThrow(
      'resolved to 169.254.169.254'
    );
  });

  it('blocks hostname that resolves to localhost', async () => {
    mockLookup.mockResolvedValueOnce([{ address: '127.0.0.1', family: 4 }] as never);
    await expect(resolveAndValidateUrl('https://internal.corp/')).rejects.toThrow(
      'resolved to 127.0.0.1'
    );
  });

  it('blocks if ANY resolved address is private (multiple records)', async () => {
    mockLookup.mockResolvedValueOnce([
      { address: '93.184.216.34', family: 4 },
      { address: '10.0.0.1', family: 4 },
    ] as never);
    await expect(resolveAndValidateUrl('https://sneaky.example.com/')).rejects.toThrow(
      'resolved to 10.0.0.1'
    );
  });

  it('rejects when DNS resolution fails', async () => {
    mockLookup.mockRejectedValueOnce(new Error('ENOTFOUND') as never);
    await expect(resolveAndValidateUrl('https://does-not-exist.invalid/')).rejects.toThrow(
      'DNS resolution failed'
    );
  });
});

// ---------------------------------------------------------------------------
// webFetchSkill — integration-level tests (DNS + axios mocked)
// ---------------------------------------------------------------------------

vi.mock('axios', () => ({
  default: {
    get: vi.fn(),
  },
}));

import axios from 'axios';

const mockAxiosGet = vi.mocked(axios.get);

const mockContext: AgentContext = {
  agentName: 'TestAgent',
  workspacePath: '/tmp/sera-test',
  tier: 1 as SecurityTier,
  manifest: {
    apiVersion: 'v1',
    kind: 'Agent',
    metadata: {
      name: 'TestAgent',
      displayName: 'Test Agent',
      icon: '',
      circle: 'test',
      tier: 1 as SecurityTier,
    },
    identity: { role: 'tester', description: 'Test agent' },
    model: { provider: 'openai', name: 'gpt-4' },
  },
  agentInstanceId: 'test-instance',
  containerId: undefined,
  sandboxManager: undefined,
  sessionId: 'test-session',
};

describe('webFetchSkill', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('returns error when url param is missing', async () => {
    const result = await webFetchSkill.handler({}, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('"url" is required');
  });

  it('blocks private IP URLs', async () => {
    const result = await webFetchSkill.handler(
      { url: 'http://169.254.169.254/latest/meta-data/' },
      mockContext
    );
    expect(result.success).toBe(false);
    expect(result.error).toContain('private/reserved IP addresses are not allowed');
    expect(mockAxiosGet).not.toHaveBeenCalled();
  });

  it('blocks non-http schemes', async () => {
    const result = await webFetchSkill.handler({ url: 'file:///etc/passwd' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('Only http and https URLs are allowed');
    expect(mockAxiosGet).not.toHaveBeenCalled();
  });

  it('blocks hostname resolving to private IP', async () => {
    mockLookup.mockResolvedValueOnce([{ address: '10.0.0.5', family: 4 }] as never);
    const result = await webFetchSkill.handler({ url: 'https://internal.corp/' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('resolved to 10.0.0.5');
    expect(mockAxiosGet).not.toHaveBeenCalled();
  });

  it('fetches a valid public URL successfully', async () => {
    mockLookup.mockResolvedValueOnce([{ address: '93.184.216.34', family: 4 }] as never);
    mockAxiosGet.mockResolvedValueOnce({
      status: 200,
      headers: { 'content-type': 'text/html' },
      data: '<html>hello</html>',
    } as never);

    const result = await webFetchSkill.handler({ url: 'https://example.com/' }, mockContext);
    expect(result.success).toBe(true);
    expect((result.data as Record<string, unknown>)['content']).toBe('<html>hello</html>');
    expect((result.data as Record<string, unknown>)['status']).toBe(200);
  });

  it('returns error when axios throws', async () => {
    mockLookup.mockResolvedValueOnce([{ address: '93.184.216.34', family: 4 }] as never);
    mockAxiosGet.mockRejectedValueOnce(new Error('Network error') as never);

    const result = await webFetchSkill.handler({ url: 'https://example.com/' }, mockContext);
    expect(result.success).toBe(false);
    expect(result.error).toContain('Network error');
  });
});
