import { Router } from 'express';
import type { Request, Response } from 'express';
import { v4 as uuidv4 } from 'uuid';
import { Orchestrator } from '../agents/Orchestrator.js';
import { AgentFactory } from '../agents/AgentFactory.js';
import { IntercomService } from '../intercom/IntercomService.js';
import type { ChatMessage } from '../agents/types.js';

class SSEIntercom extends IntercomService {
  constructor(
    private res: Response,
    private messageId: string,
    private model: string
  ) {
    super();
  }

  override async publishToken(
    agentId: string,
    token: string,
    done: boolean,
    messageId: string
  ): Promise<void> {
    if (messageId !== this.messageId) return;

    if (token) {
      const delta = {
        id: `chatcmpl-${this.messageId}`,
        object: 'chat.completion.chunk',
        created: Math.floor(Date.now() / 1000),
        model: this.model,
        choices: [
          {
            index: 0,
            delta: { content: token },
            finish_reason: null,
          },
        ],
      };
      this.res.write(`data: ${JSON.stringify(delta)}\n\n`);
    }

    if (done) {
      const finalDelta = {
        id: `chatcmpl-${this.messageId}`,
        object: 'chat.completion.chunk',
        created: Math.floor(Date.now() / 1000),
        model: this.model,
        choices: [
          {
            index: 0,
            delta: {},
            finish_reason: 'stop',
          },
        ],
      };
      this.res.write(`data: ${JSON.stringify(finalDelta)}\n\n`);
      this.res.write('data: [DONE]\n\n');
    }
  }
}

/**
 * Creates a router for OpenAI-compatible API endpoints.
 * Provides a /chat/completions endpoint that maps to SERA agents.
 */
export function createOpenAICompatRouter(orchestrator: Orchestrator) {
  const router = Router();

  router.post('/chat/completions', async (req: Request, res: Response) => {
    const { model, messages, stream } = req.body;

    if (!model) {
      return res.status(400).json({ error: { message: 'model is required' } });
    }
    if (!messages || !Array.isArray(messages) || messages.length === 0) {
      return res
        .status(400)
        .json({ error: { message: 'messages array is required and cannot be empty' } });
    }

    // Attempt to resolve agent manifest by name (model)
    const manifest = orchestrator.getManifest(model);
    if (!manifest) {
      return res.status(404).json({ error: { message: `Model/Agent "${model}" not found` } });
    }

    // Map request messages to internal ChatMessage format
    const chatMessages: ChatMessage[] = messages.map(
      (m: { role: string; content?: string; tool_calls?: unknown[]; tool_call_id?: string }) => {
        const msg: ChatMessage = {
          role: m.role as 'user' | 'assistant' | 'system' | 'tool',
          content: m.content || '',
        };
        if (m.tool_calls) msg.tool_calls = m.tool_calls;
        if (m.tool_call_id) msg.tool_call_id = m.tool_call_id;
        return msg;
      }
    );

    const lastMsg = chatMessages[chatMessages.length - 1];
    const history = chatMessages.slice(0, -1);
    const input = lastMsg?.content || '';

    if (stream) {
      const messageId = uuidv4();
      const sseIntercom = new SSEIntercom(res, messageId, model);
      const agent = AgentFactory.createAgent(manifest, undefined, sseIntercom);

      res.setHeader('Content-Type', 'text/event-stream');
      res.setHeader('Cache-Control', 'no-cache');
      res.setHeader('Connection', 'keep-alive');

      try {
        await agent.processStream(input, history, messageId);
        res.end();
      } catch (err: unknown) {
        const error = err as Error;
        if (!res.headersSent) {
          res.status(500).json({ error: { message: error.message } });
        } else {
          res.write(`data: ${JSON.stringify({ error: { message: error.message } })}\n\n`);
          res.end();
        }
      }
    } else {
      const agent = AgentFactory.createAgent(manifest);
      try {
        const response = await agent.process(input, history);
        const reply = response.finalAnswer || response.thought || 'No response generated.';

        const openAIResponse = {
          id: `chatcmpl-${uuidv4()}`,
          object: 'chat.completion',
          created: Math.floor(Date.now() / 1000),
          model,
          choices: [
            {
              index: 0,
              message: {
                role: 'assistant',
                content: reply,
              },
              finish_reason: 'stop',
            },
          ],
          usage: {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
          },
        };
        res.json(openAIResponse);
      } catch (err: unknown) {
        res.status(500).json({ error: { message: (err as Error).message } });
      }
    }
  });

  return router;
}
