/**
 * Built-in skill: knowledge-update (Issue sera-6q9)
 *
 * Find-and-replace within a personal memory block by ID.
 * Inspired by Letta's core_memory_replace(label, old_str, new_str).
 */

import type { SkillDefinition } from '../types.js';
import { ScopedMemoryBlockStore } from '../../memory/blocks/ScopedMemoryBlockStore.js';
import { EmbeddingService } from '../../services/embedding.service.js';
import { VectorService } from '../../services/vector.service.js';
import type { MemoryNamespace } from '../../services/vector.service.js';
import { AuditService } from '../../audit/AuditService.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('knowledge-update');

const MEMORY_ROOT = process.env.MEMORY_PATH ?? '/memory';

const embeddingService = EmbeddingService.getInstance();

export function createKnowledgeUpdateSkill(): SkillDefinition {
  return {
    id: 'knowledge-update',
    description:
      'Find and replace text within a personal memory block. Use to correct or extend existing knowledge.',
    source: 'builtin',
    parameters: [
      {
        name: 'blockId',
        type: 'string',
        description: 'ID of the memory block to update',
        required: true,
      },
      {
        name: 'oldText',
        type: 'string',
        description: 'Exact text to find within the block content',
        required: true,
      },
      {
        name: 'newText',
        type: 'string',
        description: 'Replacement text',
        required: true,
      },
    ],
    handler: async (params, context) => {
      const blockId = params['blockId'];
      if (typeof blockId !== 'string' || !blockId.trim()) {
        return { success: false, error: '"blockId" is required' };
      }

      const oldText = params['oldText'];
      if (typeof oldText !== 'string' || !oldText) {
        return { success: false, error: '"oldText" is required' };
      }

      const newText = params['newText'];
      if (typeof newText !== 'string') {
        return { success: false, error: '"newText" is required' };
      }

      const agentId = context.agentInstanceId ?? context.agentName;
      const store = new ScopedMemoryBlockStore(MEMORY_ROOT);

      const block = await store.readByAgent(agentId, blockId);
      if (!block) {
        return { success: false, error: `Block "${blockId}" not found` };
      }

      if (!block.content.includes(oldText)) {
        return {
          success: false,
          error: `"oldText" not found in block "${blockId}". The text must match exactly.`,
        };
      }

      const updatedContent = block.content.replace(oldText, newText);
      const updated = await store.update(agentId, blockId, { content: updatedContent });
      if (!updated) {
        return { success: false, error: `Failed to update block "${blockId}"` };
      }

      // Re-index vector
      if (embeddingService.isAvailable()) {
        try {
          const vectorService = new VectorService('_ku_unused');
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
          logger.warn(`Failed to re-index updated block ${blockId}:`, err);
        }
      }

      try {
        await AuditService.getInstance().record({
          actorType: 'agent',
          actorId: agentId,
          actingContext: null,
          eventType: 'knowledge.updated',
          payload: { blockId, type: updated.type, scope: 'personal' },
        });
      } catch (err) {
        logger.warn('Audit record failed:', err);
      }

      return { success: true, data: { id: blockId, scope: 'personal', success: true } };
    },
  };
}
