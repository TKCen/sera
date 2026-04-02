/**
 * Built-in skill: core-memory (Epic 08, Story 8.2)
 *
 * Allows agents to self-edit their core memory blocks (persona, human, etc.)
 */

import type { SkillDefinition } from '../types.js';
import { CoreMemoryService } from '../../memory/CoreMemoryService.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('core-memory-skill');

export const coreMemoryAppendSkill: SkillDefinition = {
  id: 'core_memory_append',
  description: 'Append content to a core memory block (persona, human, etc.).',
  source: 'builtin',
  parameters: [
    {
      name: 'block',
      type: 'string',
      description: 'The name of the memory block to edit (e.g., "persona", "human")',
      required: true,
    },
    {
      name: 'content',
      type: 'string',
      description: 'The content to append to the block',
      required: true,
    },
  ],
  handler: async (params, context) => {
    const { block, content } = params;
    const agentId = context.agentInstanceId;

    if (!agentId) {
      return { success: false, error: 'Agent instance ID not available' };
    }

    try {
      const service = CoreMemoryService.getInstance();
      const existing = await service.getBlockByName(agentId, block as string);

      if (!existing) {
        return { success: false, error: `Core memory block "${block}" not found` };
      }

      const newContent = (existing.content + '\n' + content).trim();
      await service.updateBlock(agentId, block as string, newContent);

      return {
        success: true,
        data: { message: `Appended to ${block} successfully. New length: ${newContent.length}` },
      };
    } catch (err) {
      logger.error(`Failed to append to core memory block ${block}:`, err);
      return { success: false, error: (err as Error).message };
    }
  },
};

export const coreMemoryReplaceSkill: SkillDefinition = {
  id: 'core_memory_replace',
  description: 'Replace a string in a core memory block with a new string.',
  source: 'builtin',
  parameters: [
    {
      name: 'block',
      type: 'string',
      description: 'The name of the memory block to edit (e.g., "persona", "human")',
      required: true,
    },
    {
      name: 'old_content',
      type: 'string',
      description: 'The string to find and replace',
      required: true,
    },
    {
      name: 'new_content',
      type: 'string',
      description: 'The new string to insert',
      required: true,
    },
  ],
  handler: async (params, context) => {
    const { block, old_content, new_content } = params;
    const agentId = context.agentInstanceId;

    if (!agentId) {
      return { success: false, error: 'Agent instance ID not available' };
    }

    try {
      const service = CoreMemoryService.getInstance();
      const existing = await service.getBlockByName(agentId, block as string);

      if (!existing) {
        return { success: false, error: `Core memory block "${block}" not found` };
      }

      if (!existing.content.includes(old_content as string)) {
        return { success: false, error: `String "${old_content}" not found in block "${block}"` };
      }

      const updatedContent = existing.content
        .split(old_content as string)
        .join(new_content as string);
      await service.updateBlock(agentId, block as string, updatedContent);

      return {
        success: true,
        data: {
          message: `Replaced content in ${block} successfully. New length: ${updatedContent.length}`,
        },
      };
    } catch (err) {
      logger.error(`Failed to replace content in core memory block ${block}:`, err);
      return { success: false, error: (err as Error).message };
    }
  },
};
