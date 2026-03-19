/**
 * Intercom REST API routes.
 *
 *   POST /api/intercom/publish   — publish to a channel
 *   POST /api/intercom/dm        — send a direct message
 *   GET  /api/intercom/history   — retrieve channel history
 *   GET  /api/intercom/channels  — list channels for an agent
 */

import { Router } from 'express';
import type { IntercomService } from '../intercom/IntercomService.js';
import { IntercomError, IntercomPermissionError } from '../intercom/IntercomService.js';
import type { AgentManifest } from '../agents/manifest/types.js';
import type { BridgeService } from '../intercom/BridgeService.js';

export function createIntercomRouter(
  intercom: IntercomService,
  resolveManifest: (agentName: string) => AgentManifest | undefined,
  bridge?: BridgeService,
): Router {
  const router = Router();

  /**
   * Publish a raw message to a channel.
   * @param req Express request containing agent, channel, type, and payload in body
   * @param res Express response
   * @returns {Promise<void>}
   */
  router.post('/publish', async (req, res) => {
    try {
      const { agent, channel, type, payload } = req.body as {
        agent?: string;
        channel?: string;
        type?: string;
        payload?: Record<string, unknown>;
      };

      if (!agent || !channel || !type || !payload) {
        return res.status(400).json({
          error: 'Required fields: agent, channel, type, payload',
        });
      }

      const manifest = resolveManifest(agent);
      if (!manifest) {
        return res.status(404).json({ error: `Agent "${agent}" not found` });
      }

      const msg = await intercom.publishMessage(
        manifest.metadata.name,
        manifest.metadata.circle ?? '',
        channel,
        type as any,
        payload,
        { securityTier: manifest.metadata.tier },
      );

      res.json({ success: true, message: msg });
    } catch (err) {
      if (err instanceof IntercomError) {
        return res.status(403).json({ error: err.message });
      }
      const message = err instanceof Error ? err.message : String(err);
      res.status(500).json({ error: message });
    }
  });

  /**
   * Send a direct message to another agent.
   * @param req Express request containing from, to, and payload in body
   * @param res Express response
   * @returns {Promise<void>}
   */
  router.post('/dm', async (req, res) => {
    try {
      const { from, to, payload } = req.body as {
        from?: string;
        to?: string;
        payload?: Record<string, unknown>;
      };

      if (!from || !to || !payload) {
        return res.status(400).json({
          error: 'Required fields: from, to, payload',
        });
      }

      const manifest = resolveManifest(from);
      if (!manifest) {
        return res.status(404).json({ error: `Agent "${from}" not found` });
      }

      const msg = await intercom.sendDirectMessage(manifest, to, payload);
      res.json({ success: true, message: msg });
    } catch (err) {
      if (err instanceof IntercomPermissionError) {
        return res.status(403).json({ error: err.message });
      }
      const message = err instanceof Error ? err.message : String(err);
      res.status(500).json({ error: message });
    }
  });

  /**
   * Retrieve channel history.
   * @param req Express request containing channel and optional limit in query
   * @param res Express response
   * @returns {Promise<void>}
   */
  router.get('/history', async (req, res) => {
    try {
      const { channel, limit } = req.query as {
        channel?: string;
        limit?: string;
      };

      if (!channel) {
        return res.status(400).json({ error: 'Query param "channel" is required' });
      }

      const history = await intercom.getHistory(
        channel,
        limit ? parseInt(limit, 10) : undefined,
      );

      res.json({ channel, messages: history });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      res.status(500).json({ error: message });
    }
  });

  /**
   * List channels the given agent can interact with.
   * @param req Express request containing agent in query
   * @param res Express response
   * @returns {void}
   */
  router.get('/channels', (req, res) => {
    const { agent } = req.query as { agent?: string };

    if (!agent) {
      return res.status(400).json({ error: 'Query param "agent" is required' });
    }

    const manifest = resolveManifest(agent);
    if (!manifest) {
      return res.status(404).json({ error: `Agent "${agent}" not found` });
    }

    const channels = intercom.getAgentChannels(manifest);
    res.json(channels);
  });

  /**
   * Receive a bridged message from a remote instance.
   * @param req Express request containing channel and message in body
   * @param res Express response
   * @returns {Promise<void>}
   */
  router.post('/bridge/receive', async (req, res) => {
    try {
      if (!bridge) {
        return res.status(501).json({ error: 'Bridge service not enabled' });
      }

      const { channel, message } = req.body;
      if (!channel || !message) {
        return res.status(400).json({ error: 'Required fields: channel, message' });
      }

      await bridge.receive(channel, message);
      res.json({ success: true });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      res.status(500).json({ error: msg });
    }
  });

  /**
   * Get a Centrifugo connection token for an agent.
   * Story 9.5: Subscription token issuance.
   */
  router.get('/centrifugo/token', async (req, res) => {
    try {
      const { agentId } = req.query as { agentId?: string };
      if (!agentId) {
        return res.status(400).json({ error: 'agentId query param is required' });
      }

      const token = await intercom.generateConnectionToken(agentId);
      res.json({ token });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      res.status(500).json({ error: msg });
    }
  });

  /**
   * Get a Centrifugo subscription token for a channel.
   * Story 9.5: Role-based subscription token issuance.
   */
  router.get('/centrifugo/subscription', async (req, res) => {
    try {
      const { channel } = req.query as { channel?: string };
      if (!channel) {
        return res.status(400).json({ error: 'channel query param is required' });
      }

      // Role is extracted from req.operator (added by authMiddleware)
      const role = req.operator?.roles[0] || 'viewer';
      const userId = req.operator?.sub || 'anonymous';

      const token = await intercom.generateSubscriptionToken(userId, channel, role);
      res.json({ token });
    } catch (err) {
      if (err instanceof IntercomError) {
        return res.status(403).json({ error: err.message });
      }
      const msg = err instanceof Error ? err.message : String(err);
      res.status(500).json({ error: msg });
    }
  });

  return router;
}
