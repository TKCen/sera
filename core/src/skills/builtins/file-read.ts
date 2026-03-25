import fs from 'fs/promises';
import path from 'path';
import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: file-read
 *
 * Reads a file from the filesystem and returns its content with optional
 * line-based pagination (offset, limit, head, tail, range).
 */
export const fileReadSkill: SkillDefinition = {
  id: 'file-read',
  description:
    'Read a file from the filesystem. Supports line-based pagination with offset/limit, head, tail, or range.',
  source: 'builtin',
  parameters: [
    {
      name: 'path',
      type: 'string',
      description: 'Absolute or relative path to the file',
      required: true,
    },
    {
      name: 'offset',
      type: 'number',
      description: 'Start from line N (1-indexed, default: 1)',
      required: false,
    },
    {
      name: 'limit',
      type: 'number',
      description: 'Max lines to return (default: 200)',
      required: false,
    },
    {
      name: 'head',
      type: 'number',
      description: 'Return first N lines (shorthand for offset=1, limit=N)',
      required: false,
    },
    {
      name: 'tail',
      type: 'number',
      description: 'Return last N lines',
      required: false,
    },
    {
      name: 'range',
      type: 'string',
      description: 'Line range like "10-50", "100-", or "-20"',
      required: false,
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

      // ── Read raw content ──────────────────────────────────────────────
      let rawContent: string;

      if (context.containerId && context.sandboxManager) {
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
        rawContent = result.output;
      } else {
        rawContent = await fs.readFile(resolvedPath, 'utf-8');
      }

      // ── Apply line-based pagination ───────────────────────────────────
      const allLines = rawContent.split('\n');
      const totalLines = allLines.length;
      const sliced = sliceLines(allLines, params);
      const truncated = sliced.lines.length < totalLines;

      return {
        success: true,
        data: {
          path: resolvedPath,
          content: sliced.lines.join('\n'),
          totalLines,
          returnedRange: sliced.range,
          truncated,
        },
      };
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  },
};

const DEFAULT_LIMIT = 200;

/**
 * Resolve pagination parameters and slice the lines array.
 * Priority: range > head > tail > offset+limit
 */
function sliceLines(
  lines: string[],
  params: Record<string, unknown>
): { lines: string[]; range: [number, number] } {
  const total = lines.length;

  // range takes precedence: "10-50", "100-", "-20"
  if (typeof params['range'] === 'string') {
    const match = params['range'].match(/^(\d+)?-(\d+)?$/);
    if (match) {
      const from = match[1] ? Math.max(1, parseInt(match[1])) : 1;
      const to = match[2] ? Math.min(total, parseInt(match[2])) : total;
      const sliced = lines.slice(from - 1, to);
      return { lines: sliced, range: [from, Math.min(from + sliced.length - 1, total)] };
    }
  }

  // head: first N lines
  if (typeof params['head'] === 'number' && params['head'] > 0) {
    const n = Math.min(params['head'], total);
    return { lines: lines.slice(0, n), range: [1, n] };
  }

  // tail: last N lines
  if (typeof params['tail'] === 'number' && params['tail'] > 0) {
    const n = Math.min(params['tail'], total);
    const from = total - n + 1;
    return { lines: lines.slice(-n), range: [from, total] };
  }

  // offset + limit (defaults)
  const offset = typeof params['offset'] === 'number' ? Math.max(1, params['offset']) : 1;
  const limit = typeof params['limit'] === 'number' ? Math.max(1, params['limit']) : DEFAULT_LIMIT;
  const from = offset;
  const to = Math.min(from + limit - 1, total);
  const sliced = lines.slice(from - 1, to);

  return { lines: sliced, range: [from, Math.min(from + sliced.length - 1, total)] };
}
