/**
 * Built-in skills: core_memory_append, core_memory_replace (Epic 08)
 */

import type { SkillDefinition } from '../types.js';
import { CoreMemoryService } from '../../memory/CoreMemoryService.js';
import { pool } from '../../lib/database.js';
import { AuditService } from '../../audit/AuditService.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('core-memory-skills');

export function createCoreMemoryAppendSkill(): SkillDefinition {
  return {
    id: 'core_memory_append',
    description: 'Append content to a named core memory block (e.g., persona, human).',
    source: 'builtin',
    parameters: [
      {
        name: 'block',
        type: 'string',
        description: 'The name of the memory block to append to.',
        required: true,
      },
      {
        name: 'content',
        type: 'string',
        description: 'The content to append.',
        required: true,
      },
    ],
    handler: async (params, context) => {
      const { block, content } = params as { block: string; content: string };
      const agentId = context.agentInstanceId;

      if (!agentId) {
        return { success: false, error: 'Agent instance ID not found in context.' };
      }

      try {
        const coreMemoryService = CoreMemoryService.getInstance(pool);
        const updated = await coreMemoryService.appendBlock(agentId, block, content);

        await recordAudit(agentId, 'memory.core_append', { block, content });

        return {
          success: true,
          data: {
            block: updated.name,
            content: updated.content,
            characterCount: updated.content.length,
            characterLimit: updated.characterLimit,
          },
        };
      } catch (err: unknown) {
        return { success: false, error: (err as Error).message };
      }
    },
  };
}

export function createCoreMemoryReplaceSkill(): SkillDefinition {
  return {
    id: 'core_memory_replace',
    description: 'Replace text in a named core memory block with new text.',
    source: 'builtin',
    parameters: [
      {
        name: 'block',
        type: 'string',
        description: 'The name of the memory block to edit.',
        required: true,
      },
      {
        name: 'oldText',
        type: 'string',
        description: 'The text to be replaced.',
        required: true,
      },
      {
        name: 'newText',
        type: 'string',
        description: 'The replacement text.',
        required: true,
      },
    ],
    handler: async (params, context) => {
      const { block, oldText, newText } = params as { block: string; oldText: string; newText: string };
      const agentId = context.agentInstanceId;

      if (!agentId) {
        return { success: false, error: 'Agent instance ID not found in context.' };
      }

      try {
        const coreMemoryService = CoreMemoryService.getInstance(pool);
        const updated = await coreMemoryService.replaceInBlock(agentId, block, oldText, newText);

        await recordAudit(agentId, 'memory.core_replace', { block, oldText, newText });

        return {
          success: true,
          data: {
            block: updated.name,
            content: updated.content,
            characterCount: updated.content.length,
            characterLimit: updated.characterLimit,
          },
        };
      } catch (err: unknown) {
        return { success: false, error: (err as Error).message };
      }
    },
  };
}

async function recordAudit(
  agentId: string,
  eventType: string,
  payload: Record<string, unknown>
): Promise<void> {
  try {
    await AuditService.getInstance().record({
      actorType: 'agent',
      actorId: agentId,
      actingContext: null,
      eventType,
      payload,
    });
  } catch (err) {
    logger.warn('Audit record failed:', err);
  }
}
