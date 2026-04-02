/**
 * File operation handlers — file-read, file-write, file-list, file-delete.
 * All operations are scoped to the workspace directory.
 */

import fs from 'fs';
import path from 'path';
import { PermissionDeniedError, MAX_RESULT_BYTES } from './types.js';
import type { ToolOutputCallback } from '../centrifugo.js';

export function fileRead(
  workspacePath: string,
  filePath: string,
  onOutput?: ToolOutputCallback,
  toolCallId?: string
): string {
  const resolved = resolveSafe(workspacePath, filePath);
  if (!fs.existsSync(resolved)) {
    return `Error: File not found: ${filePath}`;
  }

  if (isBinaryFile(resolved)) {
    const buf = fs.readFileSync(resolved);
    const mime = guessMime(resolved);
    return `[binary:${mime}]\n${buf.toString('base64')}`;
  }

  const stats = fs.statSync(resolved);
  // Stream if file is > 16KB and we have a callback
  if (onOutput && toolCallId && stats.size > 16384) {
    const content = fs.readFileSync(resolved, 'utf-8');
    const chunkSize = 16384;
    for (let i = 0; i < content.length; i += chunkSize) {
      onOutput({
        toolCallId,
        toolName: 'file-read',
        type: 'progress',
        content: content.substring(i, i + chunkSize),
        done: false,
        timestamp: new Date().toISOString(),
      });
    }
    return content;
  }

  return fs.readFileSync(resolved, 'utf-8');
}

export function fileWrite(workspacePath: string, filePath: string, content: string): string {
  const resolved = resolveSafe(workspacePath, filePath);
  const dir = path.dirname(resolved);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(resolved, content, 'utf-8');
  return `File written: ${filePath} (${content.length} bytes)`;
}

export function fileList(workspacePath: string, dirPath?: string): string {
  const resolved = resolveSafe(workspacePath, dirPath || '.');

  if (!fs.existsSync(resolved)) {
    return `Error: Directory not found: ${dirPath ?? '.'}`;
  }

  const stat = fs.statSync(resolved);
  if (!stat.isDirectory()) {
    return `Error: Not a directory: ${dirPath ?? '.'}`;
  }

  const entries = fs.readdirSync(resolved, { withFileTypes: true });
  if (entries.length === 0) {
    return '(empty directory)';
  }

  const lines = entries.map((e) => {
    const type = e.isDirectory() ? 'dir' : 'file';
    let size = '-';
    if (e.isFile()) {
      try {
        const s = fs.statSync(path.join(resolved, e.name));
        size = `${s.size}`;
      } catch {
        // ignore stat errors
      }
    }
    return `${type}\t${size}\t${e.name}`;
  });

  return `type\tsize\tname\n${lines.join('\n')}`;
}

export function fileDelete(workspacePath: string, filePath: string, recursive?: boolean): string {
  const resolved = resolveSafe(workspacePath, filePath);

  if (!fs.existsSync(resolved)) {
    return `Error: File not found: ${filePath}`;
  }

  const stat = fs.statSync(resolved);

  if (stat.isDirectory()) {
    const entries = fs.readdirSync(resolved);
    if (entries.length > 0 && !recursive) {
      return `Error: Directory not empty: ${filePath} (use recursive: true to delete non-empty directories)`;
    }
    fs.rmSync(resolved, { recursive: true, force: true });
    return `Deleted directory: ${filePath}`;
  }

  fs.unlinkSync(resolved);
  return `Deleted file: ${filePath}`;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

export function resolveSafe(workspacePath: string, filePath: string): string {
  const resolved = path.resolve(workspacePath, filePath);
  if (!resolved.startsWith(workspacePath + path.sep) && resolved !== workspacePath) {
    throw new PermissionDeniedError(
      `Path traversal blocked: "${filePath}" resolves outside workspace`
    );
  }
  return resolved;
}

export function truncateOutput(content: string): string {
  if (Buffer.byteLength(content, 'utf-8') <= MAX_RESULT_BYTES) return content;
  const buf = Buffer.from(content, 'utf-8').subarray(0, MAX_RESULT_BYTES);
  return buf.toString('utf-8') + '\n\n[TRUNCATED — output exceeded 50 KB]';
}

function isBinaryFile(filePath: string): boolean {
  const ext = path.extname(filePath).toLowerCase();
  const binaryExts = new Set([
    '.png', '.jpg', '.jpeg', '.gif', '.bmp', '.webp', '.ico',
    '.pdf', '.zip', '.tar', '.gz', '.bz2', '.7z', '.rar',
    '.exe', '.dll', '.so', '.dylib', '.wasm',
    '.mp3', '.mp4', '.wav', '.ogg', '.avi', '.mov',
    '.ttf', '.otf', '.woff', '.woff2',
  ]);
  if (binaryExts.has(ext)) return true;

  try {
    const fd = fs.openSync(filePath, 'r');
    const buf = Buffer.alloc(512);
    const bytesRead = fs.readSync(fd, buf, 0, 512, 0);
    fs.closeSync(fd);
    for (let i = 0; i < bytesRead; i++) {
      if (buf[i] === 0) return true;
    }
  } catch {
    // If we can't read it, treat as text
  }

  return false;
}

function guessMime(filePath: string): string {
  const ext = path.extname(filePath).toLowerCase();
  const mimes: Record<string, string> = {
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.jpeg': 'image/jpeg',
    '.gif': 'image/gif',
    '.pdf': 'application/pdf',
    '.zip': 'application/zip',
  };
  return mimes[ext] ?? 'application/octet-stream';
}
