import axios from 'axios';
import { safeStringify } from '../json.js';

export async function httpRequest(
  url: string,
  method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' = 'GET',
  headers?: Record<string, string>,
  body?: string,
  timeout: number = 30000
): Promise<string> {
  // Respect proxy environment variables (axios does this by default via http_proxy/https_proxy)

  // Security checks similar to web-fetch
  if (!/^https?:\/\//i.test(url)) {
    return 'Error: Only http and https URLs are allowed';
  }

  // Allow sera-core and other SERA service hostnames (internal Docker network).
  // Block raw private IPs unless they resolve to a known SERA service.
  const SERA_INTERNAL_HOSTS =
    /^https?:\/\/(sera-core|sera-db|sera-centrifugo|centrifugo|sera-qdrant|sera-egress-proxy)(:\d+)?/i;
  if (
    !SERA_INTERNAL_HOSTS.test(url) &&
    /^https?:\/\/(localhost|127\.|10\.|172\.(1[6-9]|2\d|3[01])\.|192\.168\.)/i.test(url)
  ) {
    return 'Error: Fetching private/local addresses is not allowed (use service hostnames like sera-core:3001)';
  }

  try {
    let parsedBody = body;
    if (body && typeof body === 'string') {
      try {
        parsedBody = JSON.parse(body);
      } catch {
        // Keep as string if not valid JSON
      }
    }

    const response = await axios({
      url,
      method,
      headers: {
        'User-Agent': 'SERA-Agent/1.0',
        ...headers,
      },
      data: parsedBody,
      timeout,
      maxContentLength: 1024 * 1024, // 1MB cap
      responseType: 'text',
      validateStatus: () => true, // Don't throw on error status codes
    });

    const output = {
      status: response.status,
      statusText: response.statusText,
      headers: response.headers,
      data: typeof response.data === 'string' ? response.data : safeStringify(response.data),
    };

    return safeStringify(output, 2);
  } catch (err) {
    return `Error: HTTP request failed: ${err instanceof Error ? err.message : String(err)}`;
  }
}
