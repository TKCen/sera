import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: pdf-read
 *
 * Extracts text from a PDF file in the workspace. Supports page ranges
 * and text or markdown output formats.
 */
export const pdfReadSkill: SkillDefinition = {
  id: 'pdf-read',
  description:
    'Extract text content from PDF files in the workspace. Supports page range selection.',
  source: 'builtin',
  parameters: [
    {
      name: 'path',
      type: 'string',
      description: 'Relative path to the PDF file within the workspace',
      required: true,
    },
    {
      name: 'pages',
      type: 'string',
      description: 'Page range to extract, e.g., "1-5", "3", "1,3,5-7" (default: all)',
      required: false,
    },
    {
      name: 'format',
      type: 'string',
      description: 'Output format: "text" (default) or "markdown"',
      required: false,
    },
  ],
  handler: async () => {
    // This handler is a stub; the actual execution happens in the agent-runtime.
    return { success: true, data: 'PDF content extracted.' };
  },
};
