import { MemoryManager } from '../../memory/manager.js';
import type { MemoryBlockType } from '../../memory/blocks/types.js';
import { MEMORY_BLOCK_TYPES } from '../../memory/blocks/types.js';
import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: knowledge-store
 *
 * Creates a new memory entry in the block-based memory system.
 */
export function createKnowledgeStoreSkill(memoryManager: MemoryManager): SkillDefinition {
  return {
    id: 'knowledge-store',
    description: 'Store a new knowledge entry in the memory system.',
    source: 'builtin',
    parameters: [
      { name: 'title', type: 'string', description: 'Title of the memory entry', required: true },
      { name: 'content', type: 'string', description: 'Markdown content of the entry', required: true },
      { name: 'type', type: 'string', description: `Block type: ${MEMORY_BLOCK_TYPES.join(', ')}`, required: false },
      { name: 'tags', type: 'array', description: 'Array of tag strings', required: false },
      { name: 'refs', type: 'array', description: 'Array of referenced entry IDs', required: false },
    ],
    handler: async (params) => {
      const title = params['title'];
      const content = params['content'];

      if (!title || typeof title !== 'string') {
        return { success: false, error: 'Parameter "title" is required and must be a string' };
      }
      if (!content || typeof content !== 'string') {
        return { success: false, error: 'Parameter "content" is required and must be a string' };
      }

      const type: MemoryBlockType =
        typeof params['type'] === 'string' && MEMORY_BLOCK_TYPES.includes(params['type'] as MemoryBlockType)
          ? (params['type'] as MemoryBlockType)
          : 'core';

      const opts: import('../../memory/blocks/types.js').CreateEntryOptions = { title, content };
      if (Array.isArray(params['tags'])) {
        opts.tags = params['tags'] as string[];
      }
      if (Array.isArray(params['refs'])) {
        opts.refs = params['refs'] as string[];
      }

      try {
        const entry = await memoryManager.addEntry(type, opts);
        return { success: true, data: entry };
      } catch (err) {
        return {
          success: false,
          error: err instanceof Error ? err.message : String(err),
        };
      }
    },
  };
}
