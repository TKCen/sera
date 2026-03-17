import { Router } from 'express';
import { Logger } from '../lib/logger.js';
import type { PartySessionManager } from '../circles/PartyMode.js';
import type { CircleRegistry } from '../circles/CircleRegistry.js';
import type { Orchestrator } from '../agents/Orchestrator.js';

const logger = new Logger('PartyRouter');

export function createPartyRouter(
  partyManager: PartySessionManager,
  circleRegistry: CircleRegistry,
  orchestrator: Orchestrator,
) {
  const router = Router();

  /**
   * Start a new party session for a circle.
   */
  router.post('/start', async (req, res) => {
    try {
      const { circleId } = req.body;
      if (!circleId) {
        return res.status(400).json({ error: 'circleId is required' });
      }

      const circle = circleRegistry.getCircle(circleId);
      if (!circle) {
        return res.status(404).json({ error: `Circle "${circleId}" not found` });
      }

      // Collect all agents registered in the orchestrator
      // We need a Map<string, BaseAgent>
      const allAgents = new Map();
      for (const manifest of orchestrator.getAllManifests()) {
        const agent = orchestrator.getAgent(manifest.metadata.name) || orchestrator.getAgent(manifest.identity.role);
        if (agent) {
          allAgents.set(manifest.metadata.name, agent);
        }
      }

      // Determine orchestrator agent if specified
      let orchestratorAgent;
      if (circle.partyMode?.orchestrator) {
        orchestratorAgent = allAgents.get(circle.partyMode.orchestrator);
      }

      const session = partyManager.createSession(circle, allAgents, orchestratorAgent);
      res.status(201).json(session.getInfo());
    } catch (err: any) {
      logger.error('Failed to start party session:', err);
      res.status(500).json({ error: err.message });
    }
  });

  /**
   * Send a message to a party session.
   */
  router.post('/:sessionId/chat', async (req, res) => {
    try {
      const { sessionId } = req.params;
      const { message } = req.body;

      if (!message) {
        return res.status(400).json({ error: 'message is required' });
      }

      const session = partyManager.getSession(sessionId);
      if (!session) {
        return res.status(404).json({ error: `Session "${sessionId}" not found` });
      }

      const responses = await session.sendMessage(message);
      res.json({
        sessionId,
        responses,
      });
    } catch (err: any) {
      logger.error('Party chat error:', err);
      res.status(500).json({ error: err.message });
    }
  });

  /**
   * Get session history.
   */
  router.get('/:sessionId/history', (req, res) => {
    const { sessionId } = req.params;
    const session = partyManager.getSession(sessionId);
    if (!session) {
      return res.status(404).json({ error: `Session "${sessionId}" not found` });
    }
    res.json(session.getHistory());
  });

  /**
   * List active sessions.
   */
  router.get('/sessions', (req, res) => {
    const { circleId } = req.query;
    res.json(partyManager.listSessions(circleId as string));
  });

  return router;
}
