/**
 * Tool handler for image-view.
 * Reads an image from the workspace and returns its base64 data URL.
 */

import fs from 'fs';
import path from 'path';
import { resolveSafe } from './file-handlers.js';
import { PermissionDeniedError } from './types.js';

const MAX_IMAGE_SIZE_BYTES = 5 * 1024 * 1024; // 5MB

/**
 * Handle image-view tool call.
 * Returns a special JSON payload that the ReasoningLoop will intercept.
 */
export function imageView(workspacePath: string, filePath: string, prompt?: string): string {
  const resolved = resolveSafe(workspacePath, filePath);

  if (!fs.existsSync(resolved)) {
    return `Error: Image file not found: ${filePath}`;
  }

  const stat = fs.statSync(resolved);
  if (!stat.isFile()) {
    return `Error: Not a file: ${filePath}`;
  }

  if (stat.size > MAX_IMAGE_SIZE_BYTES) {
    return `Error: Image size exceeds 5MB limit: ${filePath}`;
  }

  const ext = path.extname(resolved).toLowerCase();
  const supportedMimeTypes: Record<string, string> = {
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.jpeg': 'image/jpeg',
    '.gif': 'image/gif',
    '.webp': 'image/webp',
  };

  const mimeType = supportedMimeTypes[ext];
  if (!mimeType) {
    return `Error: Unsupported image format: ${ext}. Supported formats: PNG, JPEG, GIF, WebP.`;
  }

  const data = fs.readFileSync(resolved);
  const base64 = data.toString('base64');
  const dataUrl = `data:${mimeType};base64,${base64}`;

  // Special marker payload for ReasoningLoop interception
  return JSON.stringify({
    __sera_vision_request__: true,
    dataUrl,
    prompt: prompt ?? 'Analyze this image.',
    path: filePath,
  });
}
