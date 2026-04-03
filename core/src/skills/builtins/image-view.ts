import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: image-view
 *
 * Reads an image file from the workspace and prepares it for vision analysis.
 */
export const imageViewSkill: SkillDefinition = {
  id: 'image-view',
  description:
    'Pass an image to a vision-capable model for analysis. Returns a vision content block.',
  source: 'builtin',
  parameters: [
    {
      name: 'path',
      type: 'string',
      description: 'Workspace-scoped path to the image file (PNG, JPEG, GIF, WebP)',
      required: true,
    },
    {
      name: 'prompt',
      type: 'string',
      description: 'Optional prompt or question about the image',
      required: false,
    },
  ],
  handler: async (params, context) => {
    // The actual processing happens in the agent-runtime ReasoningLoop
    // to correctly inject the multi-modal block into the conversation.
    // This handler returns metadata for the runtime to act upon.
    return {
      success: true,
      data: {
        path: params['path'],
        prompt: params['prompt'],
        __type: 'vision_request',
      },
    };
  },
};
