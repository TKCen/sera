import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: code-eval
 *
 * Executes JavaScript/TypeScript code in a sandboxed environment within the agent container.
 * Useful for data manipulation, logic, and computations.
 */
export const codeEvalSkill: SkillDefinition = {
  id: 'code-eval',
  description:
    'Execute JavaScript/TypeScript code in a sandboxed context within the agent container.',
  source: 'builtin',
  parameters: [
    {
      name: 'code',
      type: 'string',
      description: 'The code to execute',
      required: true,
    },
    {
      name: 'language',
      type: 'string',
      description: 'The programming language: "javascript" (default) or "typescript"',
      required: false,
    },
    {
      name: 'timeout',
      type: 'number',
      description: 'Execution timeout in milliseconds (default: 5000, max: 30000)',
      required: false,
    },
  ],
  handler: async () => {
    // This handler is a stub; the actual execution happens in the agent-runtime.
    return { success: true, data: 'Code execution result.' };
  },
};
