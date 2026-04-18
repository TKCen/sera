import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: web-search
 *
 * Performs a web search using the DuckDuckGo HTML search endpoint.
 * The DDG Instant Answer JSON API returns empty results for most queries;
 * scraping the HTML endpoint is the reliable alternative (no API key required).
 */
export const webSearchSkill: SkillDefinition = {
  id: 'web-search',
  description: 'Search the web for information using a query string.',
  source: 'builtin',
  parameters: [
    { name: 'query', type: 'string', description: 'The search query', required: true },
    {
      name: 'limit',
      type: 'number',
      description: 'Max number of results to return (default 5)',
      required: false,
    },
  ],
  handler: async (params, _context) => {
    const query = params['query'];
    if (!query || typeof query !== 'string') {
      return { success: false, error: 'Parameter "query" is required and must be a string' };
    }

    // Prevent direct URL or IP address queries to mitigate SSRF
    if (/^(https?:\/\/|[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)/i.test(query)) {
      return {
        success: false,
        error: 'Direct URLs and IP addresses are not allowed in search queries',
      };
    }

    const limit = typeof params['limit'] === 'number' ? Math.min(params['limit'], 20) : 5;

    try {
      const { default: axios } = await import('axios');

      // DDG HTML search endpoint — returns rich results even for queries that
      // the Instant Answer API returns nothing for.
      const response = await axios.get('https://html.duckduckgo.com/html/', {
        params: { q: query },
        timeout: 15_000,
        headers: {
          // DDG blocks requests without a user-agent
          'User-Agent': 'Mozilla/5.0 (compatible; SERA-Agent/1.0)',
          Accept: 'text/html,application/xhtml+xml,*/*',
          'Accept-Language': 'en-US,en;q=0.9',
        },
        responseType: 'text',
      });

      const html: string = response.data as string;
      const results: { title: string; url: string; text: string }[] = [];

      // ── Parse result blocks ──────────────────────────────────────────────────
      // Each organic result is wrapped in a <div class="result results_links ...">
      // We extract title, URL, and snippet with lightweight regex — no cheerio needed.

      // Split by result blocks
      const blockPattern = /<a[^>]+class="result__a"[^>]*href="([^"]*)"[^>]*>([\s\S]*?)<\/a>/g;
      const snippetPattern = /<a[^>]+class="result__snippet"[^>]*>([\s\S]*?)<\/a>/g;

      const titleMatches = [...html.matchAll(blockPattern)];
      const snippetMatches = [...html.matchAll(snippetPattern)];

      for (let i = 0; i < Math.min(titleMatches.length, limit); i++) {
        const titleMatch = titleMatches[i];
        if (!titleMatch) continue;

        // DDG encodes the real URL in a redirect param — extract it
        let url = titleMatch[1] ?? '';
        const uddgMatch = url.match(/uddg=([^&]+)/);
        if (uddgMatch?.[1]) {
          url = decodeURIComponent(uddgMatch[1]);
        }

        const rawTitle = (titleMatch[2] ?? '').replace(/<[^>]+>/g, '').trim();
        const rawSnippet = (snippetMatches[i]?.[1] ?? '').replace(/<[^>]+>/g, '').trim();

        if (!rawTitle && !url) continue;

        results.push({
          title: rawTitle || url,
          url,
          text: rawSnippet || rawTitle,
        });
      }

      if (results.length === 0) {
        return {
          success: true,
          data: [],
          message: 'No results found for this query.',
        };
      }

      return { success: true, data: results };
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  },
};
