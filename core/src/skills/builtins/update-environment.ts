import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: update-environment
 *
 * Allows an agent to rebuild its own sandbox environment with new dependencies.
 * After building, the agent will typically need to be restarted with the new image.
 */
export const updateEnvironmentSkill: SkillDefinition = {
  id: 'update-environment',
  description: 'Rebuild the agent sandbox environment with a new Dockerfile. Useful for installing missing toolchains (Python, Rust, etc.).',
  source: 'builtin',
  parameters: [
    {
      name: 'dockerfile',
      type: 'string',
      description: 'The full content of the Dockerfile to build. Should usually extend the current image.',
      required: true
    },
  ],
  handler: async (params, context) => {
    const dockerfile = params['dockerfile'];
    if (!dockerfile || typeof dockerfile !== 'string') {
      return { success: false, error: 'Parameter "dockerfile" is required and must be a string' };
    }

    if (!context.sandboxManager) {
      return { success: false, error: 'SandboxManager not available in agent context' };
    }

    if (!context.manifest) {
      return { success: false, error: 'Agent manifest not available in agent context' };
    }

    try {
      const tagName = await context.sandboxManager.buildImage(context.manifest, dockerfile);

      return {
        success: true,
        data: {
          message: "Environment successfully built.",
          image: tagName,
          relaunchRequired: true,
          instructions: "The agent will now be restarted to apply the new environment."
        }
      };
    } catch (err: any) {
      return { success: false, error: err.message };
    }
  },
};
