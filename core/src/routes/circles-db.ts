import { Router } from 'express';
import { CircleService } from '../circles/CircleService.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('CirclesDbRouter');

export function createCirclesDbRouter(orchestrator: Orchestrator) {
  const router = Router();
  const circleService = CircleService.getInstance();

  // ── Story 10.1: Circle CRUD ───────────────────────────────────────────────

  router.get('/', async (_req, res) => {
    try {
      const circles = await circleService.listCircles();
      res.json(circles);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  router.post('/', async (req, res) => {
    try {
      const { name, displayName, description, constitution } = req.body;
      if (!name || !displayName) {
        return res.status(400).json({ error: 'name and displayName are required' });
      }
      const circle = await circleService.createCircle({
        name,
        displayName,
        ...(description !== undefined ? { description } : {}),
        ...(constitution !== undefined ? { constitution } : {}),
      });
      res.status(201).json(circle);
    } catch (err: unknown) {
      const error = err as { message?: string; code?: string };
      if (error.message?.includes('unique') || error.code === '23505') {
        return res.status(409).json({ error: `Circle name "${req.body.name}" already exists` });
      }
      res.status(500).json({ error: error.message || String(err) });
    }
  });

  router.get('/:id', async (req, res) => {
    try {
      const circle = await circleService.getCircle(req.params.id!);
      if (!circle) return res.status(404).json({ error: 'Circle not found' });
      const members = await circleService.getMembers(circle.id);
      res.json({ ...circle, members });
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  router.put('/:id', async (req, res) => {
    try {
      const { displayName, description, constitution } = req.body;
      const updated = await circleService.updateCircle(req.params.id!, {
        ...(displayName !== undefined ? { displayName } : {}),
        ...(description !== undefined ? { description } : {}),
        ...(constitution !== undefined ? { constitution } : {}),
      });
      if (!updated) return res.status(404).json({ error: 'Circle not found' });
      res.json(updated);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  router.delete('/:id', async (req, res) => {
    try {
      await circleService.deleteCircle(req.params.id!);
      res.json({ success: true });
    } catch (err: unknown) {
      if (err instanceof Error && err.message?.includes('active agent')) {
        return res.status(409).json({ error: err.message });
      }
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  router.get('/:id/members', async (req, res) => {
    try {
      const circle = await circleService.getCircle(req.params.id!);
      if (!circle) return res.status(404).json({ error: 'Circle not found' });
      const members = await circleService.getMembers(circle.id);
      res.json(members);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Story 10.6: Party Mode ────────────────────────────────────────────────

  router.post('/:id/party', async (req, res) => {
    try {
      const circle = await circleService.getCircle(req.params.id!);
      if (!circle) return res.status(404).json({ error: 'Circle not found' });

      const { prompt, participantAgentIds, rounds: maxRounds = 1 } = req.body;
      if (!prompt || typeof prompt !== 'string') {
        return res.status(400).json({ error: 'prompt is required' });
      }
      if (!Array.isArray(participantAgentIds) || participantAgentIds.length === 0) {
        return res.status(400).json({ error: 'participantAgentIds must be a non-empty array' });
      }
      const clampedRounds = Math.min(Math.max(1, maxRounds), 3);

      const session = await circleService.createPartySession({
        circleId: circle.id,
        prompt,
      });

      const intercom = orchestrator.getIntercom();

      runPartySession(
        session.id,
        circle.id,
        prompt,
        participantAgentIds as string[],
        clampedRounds,
        orchestrator,
        intercom,
        circleService
      ).catch((err) => logger.error(`Party session ${session.id} failed:`, err));

      res.status(202).json({ sessionId: session.id, circleId: circle.id, status: 'started' });
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  router.get('/:id/party/:sessionId', async (req, res) => {
    try {
      const session = await circleService.getPartySession(req.params.sessionId!);
      if (!session || session.circleId !== req.params.id!) {
        return res.status(404).json({ error: 'Party session not found' });
      }
      res.json(session);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  return router;
}

async function runPartySession(
  sessionId: string,
  circleId: string,
  prompt: string,
  participantAgentIds: string[],
  maxRounds: number,
  orchestrator: Orchestrator,
  intercom: import('../intercom/IntercomService.js').IntercomService | undefined,
  circleService: CircleService
): Promise<void> {
  const conversationHistory: string[] = [];

  for (let round = 0; round < maxRounds; round++) {
    for (const agentId of participantAgentIds) {
      const agent = orchestrator.getAgent(agentId);
      if (!agent) continue;

      const contextPrompt =
        conversationHistory.length > 0
          ? `${prompt}\n\nPrevious responses:\n${conversationHistory.join('\n')}\n\nYour response:`
          : prompt;

      let response = '';
      try {
        const result = await agent.process(contextPrompt);
        response = result.finalAnswer ?? result.thought ?? '';
      } catch (err) {
        response = `[Error: ${(err as Error).message}]`;
      }

      const partyRound = {
        agentId,
        response,
        timestamp: new Date().toISOString(),
      };

      await circleService.appendPartyRound(sessionId, partyRound);

      conversationHistory.push(`${agentId}: ${response}`);

      if (intercom) {
        await intercom
          .publish(`circle:${circleId}`, {
            type: 'party.round',
            sessionId,
            agentId,
            response,
            round,
          })
          .catch(() => {});
      }
    }
  }

  await circleService.completePartySession(sessionId);
}
