import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: image-view
 *
 * Reads an image from the workspace and passes it to the reasoning loop
 * for inclusion in the conversation as a vision content block.
 */
export const imageViewSkill: SkillDefinition = {
  id: 'image-view',
  description:
    'View an image file from the workspace to analyze its content with vision-capable models.',
  source: 'builtin',
  parameters: [
    {
      name: 'path',
      type: 'string',
      description: 'Relative path to the image file within the workspace',
      required: true,
    },
    {
      name: 'prompt',
      type: 'string',
      description: 'What to look for or analyze in the image (optional)',
      required: false,
    },
  ],
  handler: async () => {
    // This handler is a stub; the actual execution happens in the agent-runtime
    // which intercepts this tool call and injects the image into the conversation.
    return { success: true, data: 'Image processed and added to conversation.' };
  },
};
