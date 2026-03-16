import fs from 'fs/promises';
import path from 'path';
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
    const rawPath = params['path'];
    if (!rawPath || typeof rawPath !== 'string') {
      return { success: false, error: 'Parameter "path" is required and must be a string' };
    }

    try {
      const workspaceDir = process.env.WORKSPACE_DIR || process.cwd();
      const resolvedPath = path.resolve(workspaceDir, rawPath);
      const rootPath = path.resolve(workspaceDir);

      if (resolvedPath !== rootPath && !resolvedPath.startsWith(rootPath + path.sep)) {
        return { success: false, error: 'Path traversal detected' };
      }

      const content = await fs.readFile(resolvedPath, 'utf-8');
      return { success: true, data: { path: resolvedPath, content } };
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  },
};
