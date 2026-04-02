/**
 * Chat Server — lightweight HTTP endpoint for interactive web chat.
 *
 * Runs alongside the stdin/polling task system. Accepts chat messages
 * from sera-core and routes them through the ReasoningLoop, which calls
 * /v1/llm/chat/completions (getting full ContextAssembler enrichment).
 *
 * Thoughts and tokens stream through Centrifugo (existing pipeline).
 * This server returns the final result as JSON.
 */

import http from 'http';
import { v4 as uuidv4 } from 'uuid';
import type { ReasoningLoop, TaskOutput } from './loop.js';
import type { ChatMessage } from './llmClient.js';
import { log } from './logger.js';

const CHAT_PORT = parseInt(process.env['AGENT_CHAT_PORT'] || '3100', 10);

interface ChatRequest {
  message: string;
  sessionId: string;
  history?: ChatMessage[];
  messageId?: string;
}

interface ChatResponse {
  result: string | null;
  thoughts: TaskOutput['thoughtStream'];
  usage: TaskOutput['usage'];
  citations?: TaskOutput['citations'];
  error?: string;
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on('data', (chunk: Buffer) => chunks.push(chunk));
    req.on('end', () => resolve(Buffer.concat(chunks).toString()));
    req.on('error', reject);
  });
}

function sendJson(res: http.ServerResponse, status: number, data: unknown): void {
  const body = JSON.stringify(data);
  res.writeHead(status, { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body) });
  res.end(body);
}

/**
 * Start the chat HTTP server. Returns a cleanup function.
 */
export function startChatServer(
  loop: ReasoningLoop,
  onBusy: () => boolean,
): { port: number; stop: () => void } {
  let ready = true;

  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url ?? '/', `http://localhost:${CHAT_PORT}`);

    // Health check
    if (req.method === 'GET' && url.pathname === '/health') {
      sendJson(res, 200, { ready, busy: onBusy() });
      return;
    }

    // Boot context preview
    if (req.method === 'GET' && url.pathname === '/boot-context') {
      sendJson(res, 200, { content: (loop as unknown as { bootContext: string }).bootContext || '' });
      return;
    }

    // Chat endpoint
    if (req.method === 'POST' && url.pathname === '/chat') {
      if (!ready) {
        sendJson(res, 503, { error: 'Agent not ready' });
        return;
      }

      let body: ChatRequest;
      try {
        const raw = await readBody(req);
        body = JSON.parse(raw) as ChatRequest;
      } catch {
        sendJson(res, 400, { error: 'Invalid JSON body' });
        return;
      }

      if (!body.message) {
        sendJson(res, 400, { error: 'message is required' });
        return;
      }

      const taskId = body.messageId || uuidv4();
      log('info', `Chat request received (taskId=${taskId}): "${body.message.substring(0, 80)}..."`);

      try {
        const output = await loop.run({
          taskId,
          task: body.message,
          history: body.history,
        });

        const response: ChatResponse = {
          result: output.result,
          thoughts: output.thoughtStream,
          usage: output.usage,
          ...(output.citations ? { citations: output.citations } : {}),
          ...(output.error ? { error: output.error } : {}),
        };

        log('info', `Chat response sent (taskId=${taskId}, tokens=${output.usage.totalTokens})`);
        sendJson(res, 200, response);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        log('error', `Chat request failed: ${msg}`);
        sendJson(res, 500, { error: msg });
      }
      return;
    }

    sendJson(res, 404, { error: 'Not found' });
  });

  server.listen(CHAT_PORT, '0.0.0.0', () => {
    log('info', `Chat server listening on port ${CHAT_PORT}`);
  });

  return {
    port: CHAT_PORT,
    stop: () => {
      ready = false;
      server.close();
    },
  };
}
