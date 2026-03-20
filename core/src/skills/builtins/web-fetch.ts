import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: web-fetch
 *
 * Fetches a URL and returns the text content. Uses axios with a 30s timeout
 * and a 500KB response cap. Useful for reading web pages, API responses, etc.
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

    // Block file:// and other non-HTTP protocols
    if (!/^https?:\/\//i.test(url)) {
      return { success: false, error: 'Only http and https URLs are allowed' };
    }

    // Block private IPs and localhost
    if (/^https?:\/\/(localhost|127\.|10\.|172\.(1[6-9]|2\d|3[01])\.|192\.168\.)/i.test(url)) {
      return { success: false, error: 'Fetching private/local addresses is not allowed' };
    }

    try {
      const { default: axios } = await import('axios');
      const response = await axios.get(url, {
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
