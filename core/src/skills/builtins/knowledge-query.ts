import { MemoryManager } from '../../memory/manager.js';
import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: knowledge-query
 *
 * Searches the memory system for entries matching a query string.
 */
export function createKnowledgeQuerySkill(memoryManager: MemoryManager): SkillDefinition {
  return {
    id: 'knowledge-query',
    description: 'Search the memory system for knowledge entries matching a query.',
    source: 'builtin',
    parameters: [
      { name: 'query', type: 'string', description: 'Search query string', required: true },
      { name: 'limit', type: 'number', description: 'Maximum number of results to return', required: false },
    ],
    handler: async (params, _context) => {
      const query = params['query'];
      if (!query || typeof query !== 'string') {
        return { success: false, error: 'Parameter "query" is required and must be a string' };
      }

      const limit = typeof params['limit'] === 'number' ? params['limit'] : undefined;

      try {
        const results = await memoryManager.search(query, limit);
        return { success: true, data: results };
      } catch (err) {
        return {
          success: false,
          error: err instanceof Error ? err.message : String(err),
        };
      }
    },
  };
}
