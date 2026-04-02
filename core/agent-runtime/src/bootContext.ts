import fs from 'fs';
import path from 'path';
import { getEncoding } from 'js-tiktoken';
import type { RuntimeManifest } from './manifest.js';
import { log } from './logger.js';

const enc = getEncoding('cl100k_base');

function countTokens(text: string): number {
  return enc.encode(text).length;
}

/**
 * Loads boot-time context files as defined in the agent manifest.
 * Injects markdown files as labeled <boot-context> blocks.
 */
export function loadBootContext(
  manifest: RuntimeManifest,
  workspacePath: string
): string {
  const config = manifest.bootContext;
  if (!config) return '';

  const totalBudget = parseInt(process.env['BOOT_CONTEXT_BUDGET'] || '8000', 10);
  const blocks: string[] = [];
  let currentTotalTokens = 0;

  const filesToLoad: Array<{ path: string; label: string; maxTokens?: number }> = [];

  // 1. Collect individual files
  if (config.files) {
    filesToLoad.push(...config.files);
  }

  // 2. Collect files from directory
  if (config.directory) {
    const fullDir = path.join(workspacePath, config.directory);
    const resolvedDir = path.resolve(fullDir);
    const wsResolved = path.resolve(workspacePath);

    if (resolvedDir.startsWith(wsResolved + path.sep) || resolvedDir === wsResolved) {
      try {
        if (fs.existsSync(fullDir) && fs.statSync(fullDir).isDirectory()) {
          const files = fs.readdirSync(fullDir);
          for (const file of files) {
            if (file.endsWith('.md')) {
              filesToLoad.push({
                path: path.join(config.directory, file),
                label: file,
              });
            }
          }
        } else {
          log('warn', `Boot context directory not found or not a directory: ${config.directory}`);
        }
      } catch (err) {
        log('warn', `Error reading boot context directory ${config.directory}: ${err instanceof Error ? err.message : String(err)}`);
      }
    } else {
      log('warn', `Boot context directory path traversal blocked: ${config.directory}`);
    }
  }

  for (const fileSpec of filesToLoad) {
    if (currentTotalTokens >= totalBudget) {
      log('warn', `Boot context budget exceeded (${totalBudget} tokens), skipping remaining files`);
      break;
    }

    const fullPath = path.join(workspacePath, fileSpec.path);
    const resolved = path.resolve(fullPath);
    const wsResolved = path.resolve(workspacePath);

    // Path traversal check
    if (!resolved.startsWith(wsResolved + path.sep) && resolved !== wsResolved) {
      log('warn', `Boot context path traversal blocked: ${fileSpec.path}`);
      continue;
    }

    if (!fs.existsSync(fullPath)) {
      log('warn', `Boot context file not found: ${fileSpec.path}`);
      continue;
    }

    try {
      let content = fs.readFileSync(fullPath, 'utf-8');
      let tokens = countTokens(content);

      // Per-file truncation
      if (fileSpec.maxTokens !== undefined && tokens > fileSpec.maxTokens) {
        // Simple truncation by tokens (best effort)
        const encoded = enc.encode(content);
        content = enc.decode(encoded.slice(0, fileSpec.maxTokens)) + '\n... [truncated]';
        tokens = countTokens(content);
      }

      // Total budget truncation
      if (currentTotalTokens + tokens > totalBudget) {
        const remainingBudget = totalBudget - currentTotalTokens;
        if (remainingBudget > 0) {
          const encoded = enc.encode(content);
          content = enc.decode(encoded.slice(0, remainingBudget)) + '\n... [truncated]';
          tokens = countTokens(content);
        } else {
          continue;
        }
      }

      blocks.push(`<boot-context label="${fileSpec.label}">\n${content.trim()}\n</boot-context>`);
      currentTotalTokens += tokens;
    } catch (err) {
      log('warn', `Error reading boot context file ${fileSpec.path}: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  return blocks.join('\n\n');
}
