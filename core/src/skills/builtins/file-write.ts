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
    {
      name: 'path',
      type: 'string',
      description: 'Absolute or relative path to the file',
      required: true,
    },
    {
      name: 'content',
      type: 'string',
      description: 'Content to write to the file',
      required: true,
    },
  ],
  handler: async (params, context) => {
    const rawPath = params['path'];
    const content = params['content'];

    if (!rawPath || typeof rawPath !== 'string') {
      return { success: false, error: 'Parameter "path" is required and must be a string' };
    }
    if (typeof content !== 'string') {
      return { success: false, error: 'Parameter "content" is required and must be a string' };
    }

    try {
      const workspaceDir = context.workspacePath;
      const resolvedPath = path.resolve(workspaceDir, rawPath);
      const rootPath = path.resolve(workspaceDir);

      if (resolvedPath !== rootPath && !resolvedPath.startsWith(rootPath + path.sep)) {
        return { success: false, error: 'Path traversal detected' };
      }

      // ── Container Isolation ─────────────────────────────────────────────
      if (context.containerId && context.sandboxManager) {
        const relativePath = path.relative(rootPath, resolvedPath);
        const containerPath = path.posix.join('/workspace', relativePath.replace(/\\/g, '/'));
        const dirPath = path.posix.dirname(containerPath);

        // We use base64 to safely transfer content without escaping issues.
        // We use positional parameters to avoid shell injection via file paths.
        const b64 = Buffer.from(content).toString('base64');
        const script = 'mkdir -p "$1" && echo "$2" | base64 -d > "$3"';

        const result = await context.sandboxManager.exec(context.manifest, {
          containerId: context.containerId,
          agentName: context.agentName,
          command: ['sh', '-c', script, '--', dirPath, b64, containerPath],
        });

        if (result.exitCode !== 0) {
          return {
            success: false,
            error: `Container exec failed (exit ${result.exitCode}): ${result.output}`,
          };
        }
        return { success: true, data: { path: containerPath, bytesWritten: content.length } };
      }

      // ── Local Execution (Fallback) ──────────────────────────────────────
      await fs.mkdir(path.dirname(resolvedPath), { recursive: true });
      await fs.writeFile(resolvedPath, content, 'utf-8');
      return { success: true, data: { path: resolvedPath, bytesWritten: content.length } };
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  },
};
