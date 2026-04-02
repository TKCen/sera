/**
 * Tool handler for http-request.
 * Full HTTP client for API interaction.
 */

import axios from 'axios';
import { log } from '../logger.js';

const DEFAULT_TIMEOUT_MS = 30000;
const MAX_RESPONSE_SIZE_BYTES = 500 * 1024; // 500KB response cap

/**
 * Handle http-request tool call.
 */
export async function httpRequest(
  url: string,
  method: string = 'GET',
  headers: Record<string, string> = {},
  body?: string,
  timeout: number = DEFAULT_TIMEOUT_MS
): Promise<string> {
  const start = Date.now();
  const effectiveMethod = method.toUpperCase();

  // Block file:// and other non-HTTP protocols
  if (!/^https?:\/\//i.test(url)) {
    return `Error: Only http and https URLs are allowed`;
  }

  // Block private IPs and localhost (similar to web-fetch)
  if (/^https?:\/\/(localhost|127\.|10\.|172\.(1[6-9]|2\d|3[01])\.|192\.168\.)/i.test(url)) {
    return `Error: Fetching private/local addresses is not allowed`;
  }

  try {
    const response = await axios({
      url,
      method: effectiveMethod,
      headers: {
        'User-Agent': 'SERA-Agent/1.0',
        Accept: 'application/json,text/plain,*/*',
        ...headers,
      },
      data: body,
      timeout: Math.min(timeout, 120_000),
      maxContentLength: MAX_RESPONSE_SIZE_BYTES,
      // Respect proxy environment variables (HTTP_PROXY, HTTPS_PROXY, NO_PROXY)
      proxy: false, // Let axios use env vars automatically
      validateStatus: () => true, // Don't throw on error status codes
    });

    const elapsed = Date.now() - start;
    const content = typeof response.data === 'string'
      ? response.data
      : JSON.stringify(response.data, null, 2);

    const result = {
      url,
      status: response.status,
      statusText: response.statusText,
      headers: response.headers,
      body: content,
      elapsedMs: elapsed,
    };

    return JSON.stringify(result, null, 2);
  } catch (err: unknown) {
    const elapsed = Date.now() - start;
    const errorMsg = err instanceof Error ? err.message : String(err);
    log('error', `http-request failed after ${elapsed}ms: ${errorMsg}`);
    return `Error: HTTP request failed: ${errorMsg}`;
  }
}
