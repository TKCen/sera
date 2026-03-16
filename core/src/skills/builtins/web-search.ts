import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: web-search
 *
 * Performs a web search using the DuckDuckGo Instant Answer API.
 * This is a lightweight stub — swap the provider for production use.
 */
export const webSearchSkill: SkillDefinition = {
  id: 'web-search',
  description: 'Search the web for information using a query string.',
  source: 'builtin',
  parameters: [
    { name: 'query', type: 'string', description: 'The search query', required: true },
    { name: 'limit', type: 'number', description: 'Max number of results to return', required: false },
  ],
  handler: async (params) => {
    const query = params['query'];
    if (!query || typeof query !== 'string') {
      return { success: false, error: 'Parameter "query" is required and must be a string' };
    }

    try {
      // Dynamic import to avoid top-level dependency
      const { default: axios } = await import('axios');
      const response = await axios.get('https://api.duckduckgo.com/', {
        params: { q: query, format: 'json', no_redirect: 1 },
        timeout: 10_000,
      });

      const data = response.data as Record<string, unknown>;
      const limit = typeof params['limit'] === 'number' ? params['limit'] : 5;

      // Extract relevant results from the DDG instant answer response
      const results: { title: string; url: string; text: string }[] = [];

      // Abstract
      if (data['Abstract'] && typeof data['Abstract'] === 'string') {
        results.push({
          title: (data['Heading'] as string | undefined) ?? 'Abstract',
          url: (data['AbstractURL'] as string | undefined) ?? '',
          text: data['Abstract'],
        });
      }

      // Related topics
      const relatedTopics = data['RelatedTopics'];
      if (Array.isArray(relatedTopics)) {
        for (const topic of relatedTopics) {
          if (results.length >= limit) break;
          if (topic && typeof topic === 'object' && 'Text' in topic) {
            const t = topic as Record<string, unknown>;
            results.push({
              title: (t['Text'] as string | undefined)?.slice(0, 80) ?? '',
              url: (t['FirstURL'] as string | undefined) ?? '',
              text: (t['Text'] as string | undefined) ?? '',
            });
          }
        }
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
