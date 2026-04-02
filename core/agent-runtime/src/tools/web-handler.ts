/**
 * Web operation handlers — web-fetch (streaming).
 */

import axios from 'axios';
import { StringDecoder } from 'string_decoder';
import type { ToolOutputCallback } from '../centrifugo.js';
import { log } from '../logger.js';

/**
 * Fetch a URL and return its text content with streaming support.
 * Replicates security checks and response limits from core.
 */
export async function webFetchStreaming(
  url: string,
  onOutput: ToolOutputCallback,
  toolCallId: string
): Promise<string> {
  const start = Date.now();

  // Block file:// and other non-HTTP protocols
  if (!/^https?:\/\//i.test(url)) {
    throw new Error('Only http and https URLs are allowed');
  }

  // Block private IPs and localhost
  if (/^https?:\/\/(localhost|127\.|10\.|172\.(1[6-9]|2\d|3[01])\.|192\.168\.)/i.test(url)) {
    throw new Error('Fetching private/local addresses is not allowed');
  }

  try {
    const response = await axios.get(url, {
      timeout: 30_000,
      maxContentLength: 500_000,
      responseType: 'stream',
      headers: {
        'User-Agent': 'SERA-Agent/1.0',
        Accept: 'text/html,text/plain,application/json,*/*',
      },
    });

    return new Promise<string>((resolve, reject) => {
      let content = '';
      let bytesRead = 0;
      const MAX_BYTES = 500_000;
      const decoder = new StringDecoder('utf-8');
      let isDone = false;

      const finish = () => {
        if (isDone) return;
        isDone = true;
        const durationMs = Date.now() - start;
        onOutput({
          toolCallId,
          toolName: 'web-fetch',
          result: content.substring(0, 500),
          duration: durationMs,
          error: false,
          timestamp: new Date().toISOString(),
        });
        resolve(content);
      };

      response.data.on('data', (chunk: Buffer) => {
        const text = decoder.write(chunk);
        bytesRead += chunk.length;

        if (bytesRead <= MAX_BYTES) {
          content += text;
          onOutput({
            toolCallId,
            toolName: 'web-fetch',
            type: 'progress',
            content: text,
            done: false,
            timestamp: new Date().toISOString(),
          });
        } else {
          // Truncate and stop stream if it exceeds limit
          content += decoder.end();
          content += '\n\n[TRUNCATED — response exceeded 500 KB]';
          response.data.destroy();
          finish();
        }
      });

      response.data.on('end', () => {
        content += decoder.end();
        finish();
      });

      response.data.on('error', (err: Error) => {
        log('error', `webFetchStreaming error: ${err.message}`);
        reject(err);
      });
    });
  } catch (err) {
    if (axios.isAxiosError(err)) {
      throw new Error(`Fetch failed (HTTP ${err.response?.status || 'network error'}): ${err.message}`);
    }
    throw err;
  }
}
