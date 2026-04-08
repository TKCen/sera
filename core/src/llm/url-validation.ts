/**
 * URL validation for provider baseUrl — SSRF protection.
 *
 * Prevents API keys from being exfiltrated to attacker-controlled servers by
 * rejecting baseUrls that point to private/internal networks.
 *
 * Rules:
 *   - Must be a valid URL
 *   - Must use HTTPS (exception: localhost and host.docker.internal for local providers)
 *   - Must not resolve to a private/loopback/link-local IP range
 *   - localhost / 127.x / ::1 allowed only for known local providers (lmstudio, ollama)
 *   - Configurable domain allowlist via SERA_PROVIDER_URL_ALLOWLIST env var (comma-separated)
 */

/** Provider names that are permitted to use localhost/127.x endpoints. */
const LOCAL_PROVIDERS = new Set(['lmstudio', 'ollama', 'vllm', 'local', 'default']);

/**
 * IPv4 private/reserved ranges that are forbidden as provider endpoints.
 * Each entry is [networkInt, maskInt].
 */
const PRIVATE_IPV4_RANGES: [number, number][] = [
  [0x7f000000, 0xff000000], // 127.0.0.0/8   loopback
  [0x0a000000, 0xff000000], // 10.0.0.0/8    RFC-1918
  [0xac100000, 0xfff00000], // 172.16.0.0/12 RFC-1918
  [0xc0a80000, 0xffff0000], // 192.168.0.0/16 RFC-1918
  [0xa9fe0000, 0xffff0000], // 169.254.0.0/16 link-local
  [0x64400000, 0xffc00000], // 100.64.0.0/10  carrier-grade NAT
  [0x00000000, 0xff000000], // 0.0.0.0/8      this-network
  [0xc0000000, 0xffffff00], // 192.0.0.0/24   IETF protocol
];

function ipv4ToInt(ip: string): number | null {
  const parts = ip.split('.');
  if (parts.length !== 4) return null;
  let result = 0;
  for (const part of parts) {
    const n = parseInt(part, 10);
    if (isNaN(n) || n < 0 || n > 255) return null;
    result = (result << 8) | n;
  }
  // Convert to unsigned 32-bit
  return result >>> 0;
}

function isPrivateIPv4(host: string): boolean {
  const ip = ipv4ToInt(host);
  if (ip === null) return false;
  for (const [network, mask] of PRIVATE_IPV4_RANGES) {
    if ((ip & mask) === network) return true;
  }
  return false;
}

function isPrivateIPv6(host: string): boolean {
  // Strip brackets from IPv6 literals like [::1]
  const bare = host.startsWith('[') && host.endsWith(']') ? host.slice(1, -1) : host;
  const lower = bare.toLowerCase();

  // Loopback
  if (lower === '::1') return true;
  // Unspecified
  if (lower === '::') return true;
  // fc00::/7 — unique local
  if (/^f[cd]/i.test(lower)) return true;
  // fe80::/10 — link-local
  if (/^fe[89ab]/i.test(lower)) return true;

  return false;
}

/** Return the configured allowlist domains from env, lowercased. */
function getAllowlistDomains(): string[] {
  const raw = process.env.SERA_PROVIDER_URL_ALLOWLIST ?? '';
  return raw
    .split(',')
    .map((d) => d.trim().toLowerCase())
    .filter((d) => d.length > 0);
}

/**
 * Validate a provider baseUrl against SSRF rules.
 *
 * @param url      The baseUrl from the provider config.
 * @param provider The provider name (e.g. 'lmstudio', 'ollama') — used to
 *                 permit localhost for known-local providers.
 * @returns        `{ valid: true }` on success or `{ valid: false, reason }` on rejection.
 */
/**
 * Synchronous validation of provider baseUrl for SSRF protection.
 * Performs basic scheme and host checks. Does NOT resolve DNS.
 */
export function validateProviderBaseUrl(
  url: string,
  provider?: string
): { valid: true } | { valid: false; reason: string } {
  if (!url || url.trim() === '') {
    // Empty baseUrl is fine — pi-mono uses the provider's default endpoint.
    return { valid: true };
  }

  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return { valid: false, reason: `Invalid URL: "${url}"` };
  }

  const scheme = parsed.protocol; // includes the trailing colon
  const host = parsed.hostname.toLowerCase();
  const isLocalhost =
    host === 'localhost' || host === '127.0.0.1' || host === '::1' || host === '[::1]';
  const isDockerInternal = host === 'host.docker.internal';
  const isLocalProvider = provider !== undefined && LOCAL_PROVIDERS.has(provider.toLowerCase());

  // ── Allowlist check (takes precedence over everything except scheme) ─────────
  const allowlist = getAllowlistDomains();
  const hostIsAllowlisted = allowlist.some(
    (domain) => host === domain || host.endsWith(`.${domain}`)
  );

  // ── Scheme enforcement ────────────────────────────────────────────────────────
  if (scheme !== 'https:' && scheme !== 'http:') {
    return {
      valid: false,
      reason: `Provider baseUrl must use HTTP or HTTPS, got "${scheme.slice(0, -1)}"`,
    };
  }

  if (scheme === 'http:') {
    // http: is only allowed for localhost and host.docker.internal (local providers)
    if (!isLocalhost && !isDockerInternal) {
      if (!hostIsAllowlisted) {
        return {
          valid: false,
          reason:
            `Provider baseUrl must use HTTPS for non-local endpoints. Got HTTP for host "${host}". ` +
            `Add the domain to SERA_PROVIDER_URL_ALLOWLIST to permit it explicitly.`,
        };
      }
    }
  }

  // ── localhost / loopback ──────────────────────────────────────────────────────
  if (isLocalhost) {
    if (!isLocalProvider && !hostIsAllowlisted) {
      return {
        valid: false,
        reason:
          `Localhost endpoints are only permitted for local providers (lmstudio, ollama). ` +
          `Set provider to "lmstudio" or "ollama", or add the host to SERA_PROVIDER_URL_ALLOWLIST.`,
      };
    }
    return { valid: true };
  }

  // host.docker.internal is the Docker-internal gateway — treat like localhost
  if (isDockerInternal) {
    return { valid: true };
  }

  // ── Allowlisted host ──────────────────────────────────────────────────────────
  if (hostIsAllowlisted) {
    return { valid: true };
  }

  // ── Private IPv4 ranges ───────────────────────────────────────────────────────
  if (isPrivateIPv4(host)) {
    return {
      valid: false,
      reason:
        `Provider baseUrl points to a private/internal IP address "${host}", which is not permitted. ` +
        `Use a public HTTPS endpoint, or add the host to SERA_PROVIDER_URL_ALLOWLIST.`,
    };
  }

  // ── Private IPv6 ranges ───────────────────────────────────────────────────────
  if (isPrivateIPv6(host)) {
    return {
      valid: false,
      reason:
        `Provider baseUrl points to a private/internal IPv6 address "${host}", which is not permitted. ` +
        `Use a public HTTPS endpoint, or add the host to SERA_PROVIDER_URL_ALLOWLIST.`,
    };
  }

  return { valid: true };
}

/**
 * Asynchronous validation of provider baseUrl for SSRF protection.
 * Performs DNS resolution to prevent DNS rebinding and internal network probing.
 */
export async function validateProviderBaseUrlAsync(
  url: string,
  provider?: string
): Promise<{ valid: true } | { valid: false; reason: string }> {
  const syncCheck = validateProviderBaseUrl(url, provider);
  if (!syncCheck.valid) return syncCheck;

  if (!url || url.trim() === '') return { valid: true };

  const parsed = new URL(url);
  const host = parsed.hostname;

  // Skip DNS resolution for special names
  if (host === 'localhost' || host === 'host.docker.internal' || host === '127.0.0.1') {
    return { valid: true };
  }

  // Allowlist domains bypass DNS resolution check (trusted)
  const allowlist = getAllowlistDomains();
  if (allowlist.some((domain) => host === domain || host.endsWith(`.${domain}`))) {
    return { valid: true };
  }

  try {
    const { lookup } = await import('node:dns/promises');
    const { address } = await lookup(host);

    if (isPrivateIPv4(address)) {
      return {
        valid: false,
        reason: `Host "${host}" resolves to private IP "${address}", which is not permitted.`,
      };
    }

    if (isPrivateIPv6(address)) {
      return {
        valid: false,
        reason: `Host "${host}" resolves to private IPv6 "${address}", which is not permitted.`,
      };
    }
  } catch (err) {
    // If DNS fails, we reject for safety
    return {
      valid: false,
      reason: `DNS resolution failed for "${host}": ${(err as Error).message}`,
    };
  }

  return { valid: true };
}
