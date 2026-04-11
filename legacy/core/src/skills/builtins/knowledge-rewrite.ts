/**
 * Built-in skill: knowledge-rewrite (Issue sera-6q9)
 *
 * Replace the entire content of a personal memory block.
 * Inspired by Letta's rethink_memory pattern — agent rewrites a block
 * with consolidated, updated, or reorganised content.
 */

import type { SkillDefinition } from '../types.js';
import { ScopedMemoryBlockStore } from '../../memory/blocks/ScopedMemoryBlockStore.js';
import { EmbeddingService } from '../../services/embedding.service.js';
import { VectorService } from '../../services/vector.service.js';
import type { MemoryNamespace } from '../../services/vector.service.js';
import { AuditService } from '../../audit/AuditService.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('knowledge-rewrite');

const MEMORY_ROOT = process.env.MEMORY_PATH ?? '/memory';

const embeddingService = EmbeddingService.getInstance();

export function createKnowledgeRewriteSkill(): SkillDefinition {
  return {
    id: 'knowledge-rewrite',
    description:
      'Replace the entire content of a personal memory block with new consolidated content.',
    source: 'builtin',
    parameters: [
      {
        name: 'blockId',
        type: 'string',
        description: 'ID of the memory block to rewrite',
        required: true,
      },
      {
        name: 'newContent',
        type: 'string',
        description: 'New content for the block (replaces existing content entirely)',
        required: true,
      },
      {
        name: 'newTitle',
        type: 'string',
        description: 'Optional new title for the block',
        required: false,
      },
    ],
    handler: async (params, context) => {
      const blockId = params['blockId'];
      if (typeof blockId !== 'string' || !blockId.trim()) {
        return { success: false, error: '"blockId" is required' };
      }

      const newContent = params['newContent'];
      if (typeof newContent !== 'string' || !newContent.trim()) {
        return { success: false, error: '"newContent" is required and must not be empty' };
      }

      const agentId = context.agentInstanceId ?? context.agentName;
      const store = new ScopedMemoryBlockStore(MEMORY_ROOT);

      const block = await store.readByAgent(agentId, blockId);
      if (!block) {
        return { success: false, error: `Block "${blockId}" not found` };
      }

      const newTitle =
        typeof params['newTitle'] === 'string' && params['newTitle'].trim()
          ? params['newTitle']
          : undefined;

      const updates: { content: string; title?: string } = { content: newContent };
      if (newTitle !== undefined) {
        updates.title = newTitle;
      }

      const updated = await store.update(agentId, blockId, updates);
      if (!updated) {
        return { success: false, error: `Failed to rewrite block "${blockId}"` };
      }

      // Re-index vector
      if (embeddingService.isAvailable()) {
        try {
          const vectorService = new VectorService('_kr_unused');
          const namespace: MemoryNamespace = `personal:${agentId}`;
          const embedText = `${updated.title}\n${updated.content}`;
          const vector = await embeddingService.embed(embedText);
          await vectorService.upsert(blockId, namespace, vector, {
            agent_id: agentId,
            created_at: block.timestamp,
            tags: updated.tags,
            type: updated.type,
            title: updated.title,
            content: updated.content,
            importance: updated.importance,
            namespace,
          });
        } catch (err) {
          logger.warn(`Failed to re-index rewritten block ${blockId}:`, err);
        }
      }

      try {
        await AuditService.getInstance().record({
          actorType: 'agent',
          actorId: agentId,
          actingContext: null,
          eventType: 'knowledge.rewritten',
          payload: { blockId, type: updated.type, scope: 'personal' },
        });
      } catch (err) {
        logger.warn('Audit record failed:', err);
      }

      return { success: true, data: { id: blockId, scope: 'personal', success: true } };
    },
  };
}
