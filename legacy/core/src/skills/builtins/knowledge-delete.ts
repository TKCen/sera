/**
 * Built-in skill: knowledge-delete (Issue sera-6q9)
 *
 * Remove a personal memory block by ID.
 */

import type { SkillDefinition } from '../types.js';
import { ScopedMemoryBlockStore } from '../../memory/blocks/ScopedMemoryBlockStore.js';
import { VectorService } from '../../services/vector.service.js';
import type { MemoryNamespace } from '../../services/vector.service.js';
import { EmbeddingService } from '../../services/embedding.service.js';
import { AuditService } from '../../audit/AuditService.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('knowledge-delete');

const MEMORY_ROOT = process.env.MEMORY_PATH ?? '/memory';

const embeddingService = EmbeddingService.getInstance();

export function createKnowledgeDeleteSkill(): SkillDefinition {
  return {
    id: 'knowledge-delete',
    description: 'Delete a personal memory block by ID. This action is irreversible.',
    source: 'builtin',
    parameters: [
      {
        name: 'blockId',
        type: 'string',
        description: 'ID of the memory block to delete',
        required: true,
      },
    ],
    handler: async (params, context) => {
      const blockId = params['blockId'];
      if (typeof blockId !== 'string' || !blockId.trim()) {
        return { success: false, error: '"blockId" is required' };
      }

      const agentId = context.agentInstanceId ?? context.agentName;
      const store = new ScopedMemoryBlockStore(MEMORY_ROOT);

      // Read block first so we know its type for the audit log
      const block = await store.readByAgent(agentId, blockId);
      if (!block) {
        return { success: false, error: `Block "${blockId}" not found` };
      }

      const deleted = await store.delete(agentId, blockId);
      if (!deleted) {
        return { success: false, error: `Failed to delete block "${blockId}"` };
      }

      // Remove from vector index if available
      if (embeddingService.isAvailable()) {
        try {
          const vectorService = new VectorService('_kd_unused');
          const namespace: MemoryNamespace = `personal:${agentId}`;
          await vectorService.delete(blockId, namespace);
        } catch (err) {
          logger.warn(`Failed to remove block ${blockId} from vector index:`, err);
        }
      }

      try {
        await AuditService.getInstance().record({
          actorType: 'agent',
          actorId: agentId,
          actingContext: null,
          eventType: 'knowledge.deleted',
          payload: { blockId, type: block.type, scope: 'personal' },
        });
      } catch (err) {
        logger.warn('Audit record failed:', err);
      }

      return { success: true, data: { id: blockId, scope: 'personal', deleted: true } };
    },
  };
}
