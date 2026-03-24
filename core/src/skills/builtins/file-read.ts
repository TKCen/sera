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
    {
      name: 'path',
      type: 'string',
      description: 'Absolute or relative path to the file',
      required: true,
    },
  ],
  handler: async (params, context) => {
    const rawPath = params['path'];
    if (!rawPath || typeof rawPath !== 'string') {
      return { success: false, error: 'Parameter "path" is required and must be a string' };
    }

    try {
      const workspaceDir = context.workspacePath;
      const resolvedPath = path.resolve(workspaceDir, rawPath);
      const rootPath = path.resolve(workspaceDir);

      const normalizedRaw = rawPath.replace(/\\/g, '/');
      const allowedRoots = context.allowedPaths ?? ['/workspace'];

      const isInWorkspace =
        resolvedPath === rootPath || resolvedPath.startsWith(rootPath + path.sep);
      const isAllowedMount =
        context.containerId &&
        allowedRoots.some((root) => normalizedRaw === root || normalizedRaw.startsWith(root + '/'));

      if (!isInWorkspace && !isAllowedMount) {
        return { success: false, error: 'Path traversal detected' };
      }

      // ── Container Isolation ─────────────────────────────────────────────
      if (context.containerId && context.sandboxManager) {
        // For allowed mount paths, use directly; for workspace paths, resolve relative
        const containerPath = isAllowedMount
          ? normalizedRaw
          : path.posix.join(
              '/workspace',
              path.relative(rootPath, resolvedPath).replace(/\\/g, '/')
            );

        const result = await context.sandboxManager.exec(context.manifest, {
          containerId: context.containerId,
          agentName: context.agentName,
          command: ['cat', containerPath],
        });

        if (result.exitCode !== 0) {
          return {
            success: false,
            error: `Container exec failed (exit ${result.exitCode}): ${result.output}`,
          };
        }
        return { success: true, data: { path: containerPath, content: result.output } };
      }

      // ── Local Execution (Fallback) ──────────────────────────────────────
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
