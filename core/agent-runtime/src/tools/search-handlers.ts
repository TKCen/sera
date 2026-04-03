/**
 * Search operation handlers — glob, grep, and read_file (partial).
 * All operations are scoped to the workspace directory.
 */

import fs from 'fs';
import readline from 'readline';
import { spawnSync } from 'child_process';
import { resolveSafe } from './file-handlers.js';
import { log } from '../logger.js';
import type { ToolOutputCallback } from '../centrifugo.js';

/**
 * Find files using a glob pattern.
 * Uses `rg --files -g <pattern>` to find files, respecting .gitignore.
 */
export function globFiles(workspacePath: string, pattern: string): string {
  const result = spawnSync('rg', ['--files', '-g', pattern, '--'], {
    cwd: workspacePath,
    encoding: 'utf-8',
    maxBuffer: 10 * 1024 * 1024,
  });

  if (result.error) {
    log('error', `globFiles error: ${result.error.message}`);
    return JSON.stringify({ error: `ripgrep error: ${result.error.message}` });
  }

  const stdout = result.stdout || '';
  let files = stdout.split('\n').filter((f) => f.length > 0);
  files.sort();

  const total = files.length;
  if (files.length > 1000) {
    files = files.slice(0, 1000);
  }

  return JSON.stringify({
    files,
    total,
    truncated: total > 1000,
  });
}

/**
 * Search for content in files using ripgrep.
 * Modes: files_with_matches, content, count.
 */
export function grepFiles(
  workspacePath: string,
  pattern: string,
  searchPath?: string,
  mode: 'files_with_matches' | 'content' | 'count' = 'content'
): string {
  const resolved = resolveSafe(workspacePath, searchPath || '.');

  if (mode === 'files_with_matches') {
    const result = spawnSync('rg', ['-l', '--', pattern, resolved], {
      cwd: workspacePath,
      encoding: 'utf-8',
      maxBuffer: 10 * 1024 * 1024,
    });
    if (result.error) {
      log('error', `grepFiles error: ${result.error.message}`);
      return JSON.stringify({ error: `ripgrep error: ${result.error.message}` });
    }
    const files = (result.stdout || '')
      .split('\n')
      .filter((f) => f.length > 0)
      .map((f) => {
        if (f.startsWith(workspacePath)) {
          return f.substring(workspacePath.length).replace(/^[/\\]/, '');
        }
        return f;
      });
    return JSON.stringify({ files, total: files.length });
  }

  if (mode === 'count') {
    const result = spawnSync('rg', ['-c', '--', pattern, resolved], {
      cwd: workspacePath,
      encoding: 'utf-8',
      maxBuffer: 10 * 1024 * 1024,
    });
    if (result.error) {
      log('error', `grepFiles error: ${result.error.message}`);
      return JSON.stringify({ error: `ripgrep error: ${result.error.message}` });
    }
    const counts: Record<string, number> = {};
    (result.stdout || '')
      .split('\n')
      .filter((f) => f.length > 0)
      .forEach((line) => {
        const parts = line.split(':');
        if (parts.length >= 2) {
          const count = parseInt(parts.pop() || '0', 10);
          let path = parts.join(':');
          if (path.startsWith(workspacePath)) {
            path = path.substring(workspacePath.length).replace(/^[/\\]/, '');
          }
          counts[path] = count;
        } else if (parts.length === 1 && !isNaN(parseInt(parts[0], 10))) {
          // Single file count
          let path = searchPath || '.';
          counts[path] = parseInt(parts[0], 10);
        }
      });
    return JSON.stringify({ counts });
  }

  // mode === 'content'
  const result = spawnSync('rg', ['--json', '--', pattern, resolved], {
    cwd: workspacePath,
    encoding: 'utf-8',
    maxBuffer: 10 * 1024 * 1024,
  });

  if (result.error) {
    log('error', `grepFiles error: ${result.error.message}`);
    return JSON.stringify({ error: `ripgrep error: ${result.error.message}` });
  }

  const lines = (result.stdout || '').split('\n').filter((l) => l.length > 0);
  const matches: any[] = [];
  let totalMatches = 0;

  for (const line of lines) {
    try {
      const parsed = JSON.parse(line);
      if (parsed.type === 'match') {
        let path = parsed.data.path.text;
        if (path.startsWith(workspacePath)) {
          path = path.substring(workspacePath.length).replace(/^[/\\]/, '');
        }
        matches.push({
          path,
          line_number: parsed.data.line_number,
          content: parsed.data.lines.text.trimEnd(),
        });
        totalMatches++;
      }
    } catch (e) {
      // ignore
    }
  }

  return JSON.stringify({
    matches: matches.slice(0, 1000),
    total: totalMatches,
    truncated: totalMatches > 1000,
  });
}

/**
 * Read a file with optional offset and limit.
 */
export async function readFilePartial(
  workspacePath: string,
  filePath: string,
  offset: number = 1,
  limit: number = 500,
  onOutput?: ToolOutputCallback,
  toolCallId?: string
): Promise<string> {
  const resolved = resolveSafe(workspacePath, filePath);
  if (!fs.existsSync(resolved)) {
    return JSON.stringify({ error: `File not found: ${filePath}` });
  }

  const stats = fs.statSync(resolved);
  if (stats.isDirectory()) {
    return JSON.stringify({ error: `Path is a directory: ${filePath}` });
  }

  const fileStream = fs.createReadStream(resolved, { encoding: 'utf-8' });
  const rl = readline.createInterface({
    input: fileStream,
    crlfDelay: Infinity,
  });

  const startLine = Math.max(1, offset);
  const endLine = startLine + limit - 1;
  const processedLines: string[] = [];
  let currentLineNum = 0;
  let totalLines = 0;

  const MAX_LINE_LENGTH = 1000;

  for await (const line of rl) {
    currentLineNum++;
    totalLines++;
    if (currentLineNum >= startLine && currentLineNum <= endLine) {
      const contentLine =
        line.length > MAX_LINE_LENGTH
          ? line.substring(0, MAX_LINE_LENGTH) + '... [TRUNCATED]'
          : line;
      processedLines.push(contentLine);

      if (onOutput && toolCallId) {
        onOutput({
          toolCallId,
          toolName: 'read_file',
          type: 'progress',
          content: contentLine + '\n',
          done: false,
          timestamp: new Date().toISOString(),
        });
      }
    }
    // We keep counting total lines, but if the file is massive we might want to stop
    // and just report totalLines as -1 or "many". However, for consistency with other
    // tools that expect a count, let's keep going.
    // Optimization: if it's way past endLine, we could just read the rest of the stream
    // without parsing into lines if we don't care about the exact total count.
  }

  return JSON.stringify({
    content: processedLines.join('\n'),
    offset: startLine,
    limit,
    total_lines: totalLines,
    line_count: processedLines.length,
  });
}
