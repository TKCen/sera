import fs from 'fs/promises';
import path from 'path';
import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: file-write
 *
 * Writes content to a file. Creates intermediate directories automatically.
 */
export const fileWriteSkill: SkillDefinition = {
  id: 'file-write',
  description: 'Write content to a file, creating directories as needed.',
  source: 'builtin',
  parameters: [
    { name: 'path', type: 'string', description: 'Absolute or relative path to the file', required: true },
    { name: 'content', type: 'string', description: 'Content to write to the file', required: true },
  ],
  handler: async (params) => {
    const filePath = params['path'];
    const content = params['content'];

    if (!filePath || typeof filePath !== 'string') {
      return { success: false, error: 'Parameter "path" is required and must be a string' };
    }
    if (typeof content !== 'string') {
      return { success: false, error: 'Parameter "content" is required and must be a string' };
    }

    try {
      await fs.mkdir(path.dirname(filePath), { recursive: true });
      await fs.writeFile(filePath, content, 'utf-8');
      return { success: true, data: { path: filePath, bytesWritten: content.length } };
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  },
};
