import fs from 'fs/promises';
import path from 'path';
import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: file-list
 *
 * Lists files and directories in a given path. Sandboxed to the workspace directory.
 */
export const fileListSkill: SkillDefinition = {
  id: 'file-list',
  description: 'List files and directories in a given path within the workspace.',
  source: 'builtin',
  parameters: [
    {
      name: 'path',
      type: 'string',
      description: 'Relative path within the workspace to list (default: root)',
      required: false,
    },
    {
      name: 'recursive',
      type: 'boolean',
      description: 'Whether to list files recursively (default: false)',
      required: false,
    },
  ],
  handler: async (params, context) => {
    try {
      const workspaceDir = context.workspacePath;
      const rawPath = typeof params['path'] === 'string' ? params['path'] : '.';
      const recursive = params['recursive'] === true;

      const resolvedPath = path.resolve(workspaceDir, rawPath);
      const rootPath = path.resolve(workspaceDir);

      // Check if path is within workspace
      const isInWorkspace =
        resolvedPath === rootPath || resolvedPath.startsWith(rootPath + path.sep);

      // Check if path targets an allowed container mount (from agent grants)
      const normalizedRaw = rawPath.replace(/\\/g, '/');
      const allowedRoots = context.allowedPaths ?? ['/workspace'];
      const isAllowedMount =
        context.containerId &&
        allowedRoots.some((root) => normalizedRaw === root || normalizedRaw.startsWith(root + '/'));

      if (!isInWorkspace && !isAllowedMount) {
        return { success: false, error: 'Path traversal detected' };
      }

      const entries = await (async () => {
        if (context.containerId && context.sandboxManager) {
          // For allowed mount paths, use the path directly; for workspace paths, resolve relative
          const containerPath = isAllowedMount
            ? normalizedRaw
            : path.posix.join(
                '/workspace',
                path.relative(rootPath, resolvedPath).replace(/\\/g, '/')
              );
          const relativePathToRoot = isAllowedMount ? '' : path.relative(rootPath, resolvedPath);

          const findCmd = recursive
            ? [
                'find',
                containerPath,
                '-maxdepth',
                String(MAX_DEPTH),
                '-not',
                '-path',
                '*/.*',
                '-not',
                '-path',
                '*node_modules*',
                '-printf',
                '%y %p %s\\n',
              ]
            : ['ls', '-F', '--color=never', containerPath];

          const result = await context.sandboxManager.exec(context.manifest, {
            containerId: context.containerId,
            agentName: context.agentName,
            command: findCmd,
          });

          if (result.exitCode !== 0) {
            throw new Error(`Container exec failed (exit ${result.exitCode}): ${result.output}`);
          }

          if (recursive) {
            // Parse find output: "f /workspace/path 123"
            return result.output
              .split('\n')
              .filter((line) => line.trim())
              .map((line) => {
                const [typeChar, fullPath, size] = line.split(' ');
                const rel = path.posix.relative('/workspace', fullPath!);
                return {
                  name: rel,
                  type: typeChar === 'd' ? 'directory' : 'file',
                  size: size ? parseInt(size) : undefined,
                } as FileEntry;
              })
              .filter((e) => e.name && e.name !== '.');
          } else {
            // Parse ls -F output
            return result.output
              .split('\n')
              .filter((line) => line.trim())
              .map((line) => {
                const isDir = line.endsWith('/');
                const name = isDir ? line.slice(0, -1) : line;
                const rel = path.posix.join(relativePathToRoot, name);
                return {
                  name: rel,
                  type: isDir ? 'directory' : 'file',
                } as FileEntry;
              });
          }
        }
        return listDir(resolvedPath, rootPath, recursive, 0);
      })();

      return {
        success: true,
        data: {
          path: rawPath,
          entries,
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

const MAX_DEPTH = 5;
const MAX_ENTRIES = 200;

interface FileEntry {
  name: string;
  type: 'file' | 'directory';
  size?: number;
}

async function listDir(
  dirPath: string,
  rootPath: string,
  recursive: boolean,
  depth: number
): Promise<FileEntry[]> {
  if (depth > MAX_DEPTH) return [];

  const dirEntries = await fs.readdir(dirPath, { withFileTypes: true });
  const results: FileEntry[] = [];

  for (const entry of dirEntries) {
    if (results.length >= MAX_ENTRIES) break;

    // Skip hidden files and common noisy dirs
    if (entry.name.startsWith('.') || entry.name === 'node_modules') continue;

    const fullPath = path.join(dirPath, entry.name);
    const relativePath = path.relative(rootPath, fullPath);

    if (entry.isDirectory()) {
      results.push({ name: relativePath, type: 'directory' });
      if (recursive && depth < MAX_DEPTH) {
        const children = await listDir(fullPath, rootPath, true, depth + 1);
        results.push(...children);
      }
    } else if (entry.isFile()) {
      try {
        const stat = await fs.stat(fullPath);
        results.push({ name: relativePath, type: 'file', size: stat.size });
      } catch {
        results.push({ name: relativePath, type: 'file' });
      }
    }
  }

  return results;
}
