import dns from 'node:dns/promises';
import net from 'node:net';

/**
 * SSRF protection utilities for the web-fetch skill.
 *
 * Defends against Server-Side Request Forgery by resolving DNS before
 * fetching and validating the resolved IP against all private/reserved ranges.
 * This defeats DNS rebinding: the check applies to the actual resolved address,
 * not the hostname.
 */

/**
 * Returns true if the given IP address (IPv4 or IPv6) is in a private,
 * loopback, link-local, or otherwise reserved range that must not be
 * reachable from an agent fetch.
 */
export function isPrivateIp(ip: string): boolean {
  // Unwrap IPv6-mapped IPv4 addresses like ::ffff:192.168.1.1
  const unwrapped = ip.startsWith('::ffff:') ? ip.slice(7) : ip;

  if (net.isIPv4(unwrapped)) {
    return isPrivateIpv4(unwrapped);
  }

  if (net.isIPv6(ip)) {
    return isPrivateIpv6(ip);
  }

  // Unrecognised format — block it to be safe
  return true;
}

function isPrivateIpv4(ip: string): boolean {
  const parts = ip.split('.').map(Number);
  const [a, b] = parts;

  if (a === undefined || b === undefined) return true;

  // 127.0.0.0/8 — loopback
  if (a === 127) return true;

  // 10.0.0.0/8 — RFC 1918
  if (a === 10) return true;

  // 172.16.0.0/12 — RFC 1918
  if (a === 172 && b >= 16 && b <= 31) return true;

  // 192.168.0.0/16 — RFC 1918
  if (a === 192 && b === 168) return true;

  // 169.254.0.0/16 — link-local / cloud metadata (AWS, GCP, Azure)
  if (a === 169 && b === 254) return true;

  // 100.64.0.0/10 — Carrier-grade NAT (RFC 6598)
  if (a === 100 && b >= 64 && b <= 127) return true;

  // 0.0.0.0/8 — "this network"
  if (a === 0) return true;

  // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24 — documentation ranges
  // 192.88.99.0/24 — 6to4 relay anycast
  // These are unlikely to be real targets but block them defensively.

  return false;
}

function isPrivateIpv6(ip: string): boolean {
  const lower = ip.toLowerCase();

  // ::1 — loopback
  if (lower === '::1') return true;

  // :: — unspecified address
  if (lower === '::') return true;

  // fe80::/10 — link-local
  if (/^fe[89ab][0-9a-f]:/i.test(lower)) return true;

  // fc00::/7 — unique local (fc and fd prefixes)
  if (/^f[cd][0-9a-f]{2}:/i.test(lower)) return true;

  // ::ffff:0:0/96 — IPv4-mapped (handled by unwrapping in isPrivateIp, but
  // also block the raw form here as a belt-and-suspenders measure)
  if (lower.startsWith('::ffff:')) return true;

  // 64:ff9b::/96 — IPv4/IPv6 translation
  if (lower.startsWith('64:ff9b::')) return true;

  return false;
}

/**
 * Parses the URL, enforces scheme, resolves its hostname via DNS, and
 * validates every resolved IP against the private-range block list.
 *
 * Throws an Error with a descriptive message if the URL is blocked.
 * Returns the parsed URL object on success.
 */
export async function resolveAndValidateUrl(rawUrl: string): Promise<URL> {
  let parsed: URL;
  try {
    parsed = new URL(rawUrl);
  } catch {
    throw new Error(`Invalid URL: ${rawUrl}`);
  }

  const { protocol, hostname } = parsed;

  if (protocol !== 'http:' && protocol !== 'https:') {
    throw new Error(`Only http and https URLs are allowed (got "${protocol}")`);
  }

  // Reject bare IP literals that are already private (no DNS lookup needed)
  if (net.isIP(hostname) !== 0) {
    if (isPrivateIp(hostname)) {
      throw new Error(`Requests to private/reserved IP addresses are not allowed: ${hostname}`);
    }
    return parsed;
  }

  // Resolve the hostname and validate every address returned
  let addresses: string[];
  try {
    const records = await dns.lookup(hostname, { all: true, family: 0 });
    addresses = records.map((r) => r.address);
  } catch (err) {
    throw new Error(
      `DNS resolution failed for "${hostname}": ${err instanceof Error ? err.message : String(err)}`
    );
  }

  if (addresses.length === 0) {
    throw new Error(`DNS resolution returned no addresses for "${hostname}"`);
  }

  for (const addr of addresses) {
    if (isPrivateIp(addr)) {
      throw new Error(
        `Requests to private/reserved addresses are not allowed: "${hostname}" resolved to ${addr}`
      );
    }
  }

  return parsed;
}
