import axios from 'axios';
import type { SkillDefinition } from '../types.js';
import { resolveAndValidateUrl } from './ssrf.js';

/**
 * Built-in skill: web-fetch
 *
 * Fetches a URL and returns the text content. Uses axios with a 30s timeout
 * and a 500KB response cap. Useful for reading web pages, API responses, etc.
 *
 * SSRF protection: the hostname is resolved via DNS before the request is made
 * and every resolved IP is validated against the private/reserved range block
 * list. This defeats DNS rebinding attacks.
 */
export const webFetchSkill: SkillDefinition = {
  id: 'web-fetch',
  description:
    'Fetch a URL and return its text content. Useful for reading web pages, documentation, or API responses.',
  source: 'builtin',
  parameters: [{ name: 'url', type: 'string', description: 'The URL to fetch', required: true }],
  handler: async (params, _context) => {
    const url = params['url'];
    if (!url || typeof url !== 'string') {
      return { success: false, error: 'Parameter "url" is required and must be a string' };
    }

    let validatedUrl: URL;
    try {
      validatedUrl = await resolveAndValidateUrl(url);
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }

    try {
      const response = await axios.get(validatedUrl.toString(), {
        timeout: 30_000,
        maxContentLength: 500_000,
        responseType: 'text',
        headers: {
          'User-Agent': 'SERA-Agent/1.0',
          Accept: 'text/html,text/plain,application/json,*/*',
        },
      });

      const content =
        typeof response.data === 'string' ? response.data : JSON.stringify(response.data);

      return {
        success: true,
        data: {
          url,
          status: response.status,
          contentType: response.headers?.['content-type'] ?? 'unknown',
          content,
        },
      };
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  },
};
