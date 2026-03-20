import { Router } from 'express';
import { v4 as uuidv4 } from 'uuid';
import { Logger } from '../lib/logger.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import type { SessionStore } from '../sessions/SessionStore.js';
import type { ChatMessage } from '../agents/types.js';

const logger = new Logger('ChatRouter');

export function createChatRouter(sessionStore: SessionStore, orchestrator: Orchestrator) {
  const router = Router();

  /**
   * Helper: resolve or create a session and load its message history.
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
    // Create a new session
    const session = await sessionStore.createSession({ agentName, agentInstanceId });
    return { sessionId: session.id, history: [], isNew: true };
  }

  /**
   * Sends a chat message.
   */
  router.post('/chat', async (req, res) => {
    try {
      const {
        message,
        sessionId: incomingSessionId,
        agentName: incomingAgent,
        agentInstanceId,
      } = req.body;
      if (!message) {
        return res.status(400).json({ error: 'message is required' });
      }

      // Resolve agent
      let agent;
      let agentName = incomingAgent || 'architect-prime';

      if (agentInstanceId) {
        // Look up instance-specific agent
        agent = orchestrator.getAgent(agentInstanceId);
        if (!agent) {
          // Try starting it if it exists in DB but not active in memory
          try {
            agent = await orchestrator.startInstance(agentInstanceId);
          } catch {
            return res
              .status(404)
              .json({ error: `Agent instance "${agentInstanceId}" not found.` });
          }
        }
        agentName = agent.role;
      } else if (incomingAgent) {
        agent = orchestrator.getAgent(incomingAgent);
        if (!agent) {
          return res.status(404).json({ error: `Agent "${incomingAgent}" not found.` });
        }
      } else {
        agent = orchestrator.getPrimaryAgent();
        if (!agent) {
          return res
            .status(500)
            .json({ error: 'No primary agent configured. Check your AGENT.yaml manifests.' });
        }
        agentName = agent.role;
      }

      // Resolve or create session
      const { sessionId, history, isNew } = await resolveSession(
        incomingSessionId,
        agentName,
        agentInstanceId
      );

      try {
        const response = await agent.process(message, history);
        const reply = response.finalAnswer || response.thought || 'No response generated.';

        // Persist messages
        await sessionStore.addMessage({ sessionId, role: 'user', content: message });
        await sessionStore.addMessage({ sessionId, role: 'assistant', content: reply });

        // Auto-title on first exchange
        if (isNew) {
          const autoTitle = message.length > 60 ? message.substring(0, 57) + '...' : message;
          await sessionStore.updateSessionTitle(sessionId, autoTitle);
        }

        // Best-effort JSONL mirror
        sessionStore.writeJsonlMirror(agentName, sessionId).catch(() => {});

        res.json({
          sessionId,
          reply,
          thought: response.thought,
        });
      } catch (agentError: any) {
        logger.error(`[${agent.name}] Error during processing:`, agentError);
        if (agentError.name === 'AbortError' || agentError.message.includes('timeout')) {
          return res
            .status(504)
            .json({ error: `Agent "${agent.name}" timed out while processing.` });
        }
        return res
          .status(500)
          .json({ error: `LLM error from "${agent.name}": ${agentError.message}` });
      }
    } catch (error: any) {
      logger.error('Chat API error:', error);
      res.status(500).json({ error: error.message });
    }
  });

  /**
   * Streams a chat response via Centrifugo.
   */
  router.post('/chat/stream', async (req, res) => {
    try {
      const {
        message,
        sessionId: incomingSessionId,
        agentName: incomingAgent,
        agentInstanceId,
      } = req.body;
      if (!message) {
        return res.status(400).json({ error: 'message is required' });
      }

      const messageId = uuidv4();

      // Resolve agent
      let agent;
      let agentName = incomingAgent || 'architect-prime';

      if (agentInstanceId) {
        agent = orchestrator.getAgent(agentInstanceId);
        if (!agent) {
          try {
            agent = await orchestrator.startInstance(agentInstanceId);
          } catch {
            return res
              .status(404)
              .json({ error: `Agent instance "${agentInstanceId}" not found.` });
          }
        }
        agentName = agent.role;
      } else if (incomingAgent) {
        agent = orchestrator.getAgent(incomingAgent);
        if (!agent) {
          return res.status(404).json({ error: `Agent "${incomingAgent}" not found.` });
        }
      } else {
        agent = orchestrator.getPrimaryAgent();
        if (!agent) {
          return res.status(500).json({ error: 'No primary agent configured.' });
        }
        agentName = agent.role;
      }

      // Resolve or create session
      const { sessionId, history, isNew } = await resolveSession(
        incomingSessionId,
        agentName,
        agentInstanceId
      );

      // Return immediately — streaming happens via Centrifugo
      res.json({ sessionId, messageId });

      // Process in background
      try {
        const response = await agent.processStream(message, history, messageId);
        const reply = response.finalAnswer || response.thought || '';

        // Persist messages — include thoughts in assistant metadata for session restore
        await sessionStore.addMessage({ sessionId, role: 'user', content: message });
        await sessionStore.addMessage({
          sessionId,
          role: 'assistant',
          content: reply,
          ...(response.thoughts && response.thoughts.length > 0
            ? { metadata: { thoughts: response.thoughts } }
            : {}),
        });

        // Auto-title on first exchange
        if (isNew) {
          const autoTitle = message.length > 60 ? message.substring(0, 57) + '...' : message;
          await sessionStore.updateSessionTitle(sessionId, autoTitle);
        }

        // Best-effort JSONL mirror
        sessionStore.writeJsonlMirror(agentName, sessionId).catch(() => {});
      } catch (err: any) {
        logger.error(`[${agent.name}] Stream error:`, err);
      }
    } catch (error: any) {
      logger.error('Chat Stream API error:', error);
      if (!res.headersSent) {
        res.status(500).json({ error: error.message });
      }
    }
  });

  return router;
}
