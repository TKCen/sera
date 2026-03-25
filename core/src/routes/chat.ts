import { Router } from 'express';
import { v4 as uuidv4 } from 'uuid';
import { Logger } from '../lib/logger.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import type { SessionStore } from '../sessions/SessionStore.js';
import type { AgentRegistry } from '../agents/registry.service.js';
import type { SandboxManager } from '../sandbox/SandboxManager.js';
import type { ChatMessage } from '../agents/types.js';

const logger = new Logger('ChatRouter');

export function createChatRouter(
  sessionStore: SessionStore,
  orchestrator: Orchestrator,
  agentRegistry?: AgentRegistry,
  sandboxManager?: SandboxManager
) {
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
        if (!agent && agentRegistry) {
          // Fallback: look up agent instance by name in the DB and start it
          const instance = await agentRegistry.getInstanceByName(incomingAgent);
          if (instance) {
            try {
              agent = await orchestrator.startInstance(instance.id);
            } catch (startErr) {
              logger.error(`Failed to start instance for agent "${incomingAgent}":`, startErr);
            }
          }
        }
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
        // Try routing through agent container for full context assembly
        const instanceId = agent.agentInstanceId;
        const sandbox =
          instanceId && sandboxManager
            ? sandboxManager.getContainerByInstance(instanceId)
            : undefined;

        let reply = '';
        let thought: string | undefined;

        if (sandbox?.chatUrl) {
          try {
            const chatRes = await fetch(`${sandbox.chatUrl}/chat`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ message, sessionId, history }),
              signal: AbortSignal.timeout(120_000),
            });
            if (chatRes.ok) {
              const body = (await chatRes.json()) as { result: string | null; error?: string };
              reply = body.result || 'No response generated.';
            } else {
              throw new Error(`Container chat returned ${chatRes.status}`);
            }
          } catch (containerErr) {
            logger.warn(`[${agent.name}] Container chat failed, falling back:`, containerErr);
            const response = await agent.process(message, history);
            reply = response.finalAnswer || response.thought || 'No response generated.';
            thought = response.thought;
          }
        } else {
          const response = await agent.process(message, history);
          reply = response.finalAnswer || response.thought || 'No response generated.';
          thought = response.thought;
        }

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

        res.json({ sessionId, reply, thought });
      } catch (agentError: unknown) {
        const err = agentError as Error;
        logger.error(`[${agent.name}] Error during processing:`, agentError);
        if (err.name === 'AbortError' || (err.message && err.message.includes('timeout'))) {
          return res
            .status(504)
            .json({ error: `Agent "${agent.name}" timed out while processing.` });
        }
        return res
          .status(500)
          .json({ error: `LLM error from "${agent.name}": ${err.message || String(agentError)}` });
      }
    } catch (error: unknown) {
      logger.error('Chat API error:', error);
      res.status(500).json({ error: error instanceof Error ? error.message : String(error) });
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
        if (!agent && agentRegistry) {
          const instance = await agentRegistry.getInstanceByName(incomingAgent);
          if (instance) {
            try {
              agent = await orchestrator.startInstance(instance.id);
            } catch (startErr) {
              logger.error(`Failed to start instance for agent "${incomingAgent}":`, startErr);
            }
          }
        }
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
        // Try routing through agent container (full context: skills + RAG + constitution)
        const instanceId = agent.agentInstanceId;
        const sandbox =
          instanceId && sandboxManager
            ? sandboxManager.getContainerByInstance(instanceId)
            : undefined;

        let reply = '';

        if (sandbox?.chatUrl) {
          // Container path — full context assembly via ContextAssembler
          try {
            const chatRes = await fetch(`${sandbox.chatUrl}/chat`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ message, sessionId, history, messageId }),
              signal: AbortSignal.timeout(120_000),
            });
            if (chatRes.ok) {
              const body = (await chatRes.json()) as { result: string | null; error?: string };
              reply = body.result || '';
              logger.info(`[${agent.name}] Chat routed through container (${sandbox.chatUrl})`);
            } else {
              throw new Error(`Container chat returned ${chatRes.status}`);
            }
          } catch (containerErr) {
            // Fallback to in-process if container chat fails
            logger.warn(
              `[${agent.name}] Container chat failed, falling back to in-process:`,
              containerErr
            );
            const response = await agent.processStream(message, history, messageId);
            reply = response.finalAnswer || response.thought || '';
          }
        } else {
          // In-process fallback (no container or chatUrl not available)
          const response = await agent.processStream(message, history, messageId);
          reply = response.finalAnswer || response.thought || '';
        }

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
      } catch (err: unknown) {
        logger.error(`[${agent.name}] Stream error:`, err);
      }
    } catch (error: unknown) {
      logger.error('Chat Stream API error:', error);
      if (!res.headersSent) {
        res.status(500).json({ error: error instanceof Error ? error.message : String(error) });
      }
    }
  });

  return router;
}
