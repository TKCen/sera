import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: code-eval
 *
 * Executes JavaScript or TypeScript code in a restricted sandbox.
 */
export const codeEvalSkill: SkillDefinition = {
  id: 'code-eval',
  description:
    'Execute JavaScript or TypeScript code in an isolated sandbox. Ideal for data transformation, math, and logic.',
  source: 'builtin',
  parameters: [
    {
      name: 'code',
      type: 'string',
      description: 'The JS/TS code to execute',
      required: true,
    },
    {
      name: 'language',
      type: 'string',
      description: 'Language of the code: "javascript" (default) or "typescript"',
      required: false,
    },
    {
      name: 'timeout',
      type: 'number',
      description: 'Timeout in ms (default: 5000, max: 30000)',
      required: false,
    },
  ],
  handler: async (params, _context) => {
    // Execution is handled by the agent-runtime
    return {
      success: true,
      data: {
        code: params['code'],
        language: params['language'],
        timeout: params['timeout'],
      },
    };
  },
};
