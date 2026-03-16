import fs from 'fs/promises';
import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: file-read
 *
 * Reads a file from the filesystem and returns its content.
 */
export const fileReadSkill: SkillDefinition = {
  id: 'file-read',
  description: 'Read a file from the filesystem and return its content.',
  source: 'builtin',
  parameters: [
    { name: 'path', type: 'string', description: 'Absolute or relative path to the file', required: true },
  ],
  handler: async (params) => {
    const filePath = params['path'];
    if (!filePath || typeof filePath !== 'string') {
      return { success: false, error: 'Parameter "path" is required and must be a string' };
    }

    try {
      const content = await fs.readFile(filePath, 'utf-8');
      return { success: true, data: { path: filePath, content } };
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  },
};
