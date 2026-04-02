import { Router } from 'express';
import type { Request, Response } from 'express';
import { v4 as uuidv4 } from 'uuid';
import { Logger } from '../lib/logger.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import type { SessionStore } from '../sessions/SessionStore.js';
import type { AgentRegistry } from '../agents/registry.service.js';
import type { BaseAgent } from '../agents/BaseAgent.js';
import type { ChatMessage } from '../agents/types.js';

const logger = new Logger('ChatRouter');

/** Response shape from the agent-runtime chat server. */
interface ContainerChatResponse {
  result: string | null;
  error?: string;
  thoughts?: Array<{ step: string; content: string }>;
  citations?: Array<{ blockId: string; scope: string; relevance: number }>;
  usage?: { promptTokens: number; completionTokens: number; totalTokens: number };
}

// ── Shared Helpers ──────────────────────────────────────────────────────────────

/**
 * Resolve the target agent from the request body.
 * Lookup order: agentInstanceId → agentName → primary agent.
 */
async function resolveAgent(
  body: { agentInstanceId?: string; agentName?: string },
  orchestrator: Orchestrator,
  agentRegistry?: AgentRegistry
): Promise<{ agent: BaseAgent; agentName: string }> {
  if (body.agentInstanceId) {
    let agent = orchestrator.getAgent(body.agentInstanceId);
    if (!agent) {
      agent = await orchestrator.startInstance(body.agentInstanceId);
    }
    return { agent, agentName: agent.role };
  }

  if (body.agentName) {
    let agent = orchestrator.getAgent(body.agentName);
    if (!agent && agentRegistry) {
      const instance = await agentRegistry.getInstanceByName(body.agentName);
      if (instance) {
        agent = await orchestrator.startInstance(instance.id);
      }
    }
    if (!agent) {
      throw Object.assign(new Error(`Agent "${body.agentName}" not found.`), { status: 404 });
    }
    // Use the requested instance name (not the template role) so sessions
    // are linked to the correct agent instance.
    return { agent, agentName: body.agentName };
  }

  const agent = orchestrator.getPrimaryAgent();
  if (!agent) {
    throw Object.assign(
      new Error('No primary agent configured. Check your AGENT.yaml manifests.'),
      { status: 500 }
    );
  }
  return { agent, agentName: agent.role };
}

/**
 * Ensure the agent has a running container and return its chat URL.
 * Auto-starts the agent if the container is not running.
 */
async function ensureContainer(agent: BaseAgent, orchestrator: Orchestrator): Promise<string> {
  const instanceId = agent.agentInstanceId;
  if (!instanceId) {
    throw Object.assign(
      new Error(`Agent "${agent.name}" has no instance ID — cannot route to container`),
      { status: 503 }
    );
  }
  try {
    return await orchestrator.ensureContainerRunning(instanceId);
  } catch (err) {
    throw Object.assign(
      new Error(
        `Container for agent "${agent.name}" is not available: ${err instanceof Error ? err.message : String(err)}`
      ),
      { status: 503 }
    );
  }
}

/**
 * Forward a chat message to the agent container's chat server.
 * Returns the reply text and optional thought.
 */
async function forwardToContainer(
  chatUrl: string,
  payload: {
    message: string;
    sessionId: string;
    history: ChatMessage[];
    messageId?: string;
  }
): Promise<{
  reply: string;
  thought?: string;
  thoughts?: ContainerChatResponse['thoughts'];
  citations?: ContainerChatResponse['citations'];
}> {
  const res = await fetch(`${chatUrl}/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
    signal: AbortSignal.timeout(120_000),
  });

  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(`Container chat returned ${res.status}: ${text}`);
  }

  const body = (await res.json()) as ContainerChatResponse;
  return {
    reply: body.result || 'No response generated.',
    ...(body.error ? { thought: `Error: ${body.error}` } : {}),
    ...(body.thoughts?.length ? { thoughts: body.thoughts } : {}),
    ...(body.citations?.length ? { citations: body.citations } : {}),
  };
}

/**
 * Persist user/assistant messages and finalize the session (auto-title, JSONL mirror).
 */
async function persistAndFinalize(
  sessionStore: SessionStore,
  sessionId: string,
  agentName: string,
  message: string,
  reply: string,
  isNew: boolean,
  thoughts?: ContainerChatResponse['thoughts'],
  citations?: ContainerChatResponse['citations']
): Promise<void> {
  await sessionStore.addMessage({ sessionId, role: 'user', content: message });
  await sessionStore.addMessage({
    sessionId,
    role: 'assistant',
    content: reply,
    ...((thoughts?.length || citations?.length)
      ? { metadata: { ...(thoughts?.length ? { thoughts } : {}), ...(citations?.length ? { citations } : {}) } }
      : {}),
  });

  if (isNew) {
    const autoTitle = message.length > 60 ? message.substring(0, 57) + '...' : message;
    await sessionStore.updateSessionTitle(sessionId, autoTitle);
  }

  sessionStore.writeJsonlMirror(agentName, sessionId).catch(() => {});
}

// ── Router ──────────────────────────────────────────────────────────────────────

export function createChatRouter(
  sessionStore: SessionStore,
  orchestrator: Orchestrator,
  agentRegistry?: AgentRegistry
) {
  const router = Router();

  /**
   * Resolve or create a session and load its message history.
   */
  async function resolveSession(
    sessionId: string | undefined,
    agentName: string,
    agentInstanceId?: string
  ): Promise<{ sessionId: string; history: ChatMessage[]; isNew: boolean }> {
    if (sessionId) {
      const existing = await sessionStore.getSession(sessionId);
      if (existing) {
        const msgs = await sessionStore.getMessages(sessionId);
        const history: ChatMessage[] = msgs.map((m) => ({
          role: m.role as ChatMessage['role'],
          content: m.content,
        }));
        return { sessionId, history, isNew: false };
      }
    }
    const session = await sessionStore.createSession({ agentName, agentInstanceId });
    return { sessionId: session.id, history: [], isNew: true };
  }

  // ── POST /chat ──────────────────────────────────────────────────────────────
  //
  // Sends a message and waits for the full response.
  // If `stream: true` is set, returns immediately with { sessionId, messageId }
  // and the response streams via Centrifugo (same as the legacy /chat/stream).
  //

  async function chatHandler(req: Request, res: Response): Promise<void> {
    try {
      const {
        message,
        sessionId: incomingSessionId,
        agentName: incomingAgent,
        agentInstanceId,
        stream,
      } = req.body;

      if (!message) {
        res.status(400).json({ error: 'message is required' });
        return;
      }

      // 1. Resolve agent
      const { agent, agentName } = await resolveAgent(
        { agentInstanceId, agentName: incomingAgent },
        orchestrator,
        agentRegistry
      );

      // 2. Resolve or create session
      const { sessionId, history, isNew } = await resolveSession(
        incomingSessionId,
        agentName,
        agentInstanceId ?? agent.agentInstanceId
      );

      // 3. Ensure container is running
      const chatUrl = await ensureContainer(agent, orchestrator);

      if (stream) {
        // ── Streaming mode: respond immediately, process in background ──
        const messageId = uuidv4();
        res.json({ sessionId, messageId });

        // Background processing — errors are logged, not returned
        (async () => {
          try {
            const { reply, thoughts, citations } = await forwardToContainer(chatUrl, {
              message,
              sessionId,
              history,
              messageId,
            });
            await persistAndFinalize(
              sessionStore,
              sessionId,
              agentName,
              message,
              reply,
              isNew,
              thoughts,
              citations
            );
          } catch (err) {
            logger.error(`[${agent.name}] Stream processing error:`, err);
          }
        })();
      } else {
        // ── Synchronous mode: wait for response ─────────────────────────
        const { reply, thought, thoughts, citations } = await forwardToContainer(chatUrl, {
          message,
          sessionId,
          history,
        });

        await persistAndFinalize(
          sessionStore,
          sessionId,
          agentName,
          message,
          reply,
          isNew,
          thoughts,
          citations
        );
        res.json({ sessionId, reply, thought, citations });
      }
    } catch (error: unknown) {
      const err = error as Error & { status?: number };
      const status = err.status || 500;

      if (err.name === 'AbortError' || err.message?.includes('timeout')) {
        res.status(504).json({ error: 'Agent timed out while processing.' });
        return;
      }

      if (status < 500) {
        res.status(status).json({ error: err.message });
        return;
      }

      logger.error('Chat API error:', error);
      if (!res.headersSent) {
        res.status(status).json({ error: err.message || String(error) });
      }
    }
  }

  router.post('/chat', chatHandler);

  // ── POST /chat/stream (deprecated — same as POST /chat with stream: true) ──

  router.post(
    '/chat/stream',
    (req, _res, next) => {
      req.body.stream = true;
      next();
    },
    chatHandler
  );

  return router;
}
