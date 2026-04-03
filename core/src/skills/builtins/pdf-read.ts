import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: pdf-read
 *
 * Extracts text content from a PDF file.
 */
export const pdfReadSkill: SkillDefinition = {
  id: 'pdf-read',
  description: 'Extract text content from a PDF file. Supports page ranges.',
  source: 'builtin',
  parameters: [
    {
      name: 'path',
      type: 'string',
      description: 'Workspace-scoped path to the PDF file',
      required: true,
    },
    {
      name: 'pages',
      type: 'string',
      description: 'Page range, e.g., "1-5", "3", "1,3,5-7" (default: all)',
      required: false,
    },
    {
      name: 'format',
      type: 'string',
      description: 'Output format: "text" (default) or "markdown"',
      required: false,
    },
  ],
  handler: async (params, context) => {
    // Execution is handled by the agent-runtime
    return {
      success: true,
      data: {
        path: params['path'],
        pages: params['pages'],
        format: params['format'],
      },
    };
  },
};
