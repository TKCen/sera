/**
 * Agent Management Routes
 *
 * Instance CRUD, lifecycle, and template listing.
 */

import { Router } from 'express';
import type { Orchestrator } from '../agents/Orchestrator.js';
import type { AgentRegistry } from '../agents/registry.service.js';
import { AgentManifestLoader } from '../agents/manifest/AgentManifestLoader.js';
import { AgentFactory } from '../agents/AgentFactory.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('AgentRouter');

export function createAgentRouter(orchestrator: Orchestrator, agentRegistry: AgentRegistry) {
  const router = Router();

  // ── List all agent instances (primary endpoint for the web UI) ────────────
  /**
   * Returns all agent instances from the DB, enriched with template metadata
   * and live orchestrator status.
   */
  router.get('/', async (_req, res) => {
    try {
      const instances = await agentRegistry.listInstances();
      const liveAgents = new Map(orchestrator.listAgents().map((a) => [a.id, a]));

      const enriched = await Promise.all(
        instances.map(async (inst) => {
          const template = await agentRegistry.getTemplate(inst.template_ref);
          const live = liveAgents.get(inst.id);
          return {
            id: inst.id,
            name: inst.name,
            display_name: inst.display_name ?? template?.display_name,
            template_ref: inst.template_ref,
            status: live?.status ?? inst.status,
            circle: inst.circle,
            lifecycle_mode: inst.lifecycle_mode,
            icon: template?.spec?.identity?.icon ?? template?.spec?.icon,
            sandbox_boundary: template?.spec?.sandboxBoundary,
            created_at: inst.created_at,
            updated_at: inst.updated_at,
          };
        })
      );

      res.json(enriched);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── List all agent templates ───────────────────────────────────────────────
  router.get('/templates', async (_req, res) => {
    try {
      const templates = await agentRegistry.listTemplates();
      res.json(templates);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── List all agent instances (raw DB) ─────────────────────────────────────
  router.get('/instances', async (req, res) => {
    try {
      const circle = req.query.circle as string | undefined;
      const status = req.query.status as string | undefined;
      const instances = await agentRegistry.listInstances({
        ...(circle ? { circle } : {}),
        ...(status ? { status } : {}),
      });
      res.json(instances);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Create a new agent instance ────────────────────────────────────────────
  /**
   * Creates a new agent instance from a template.
   * POST /api/agents/instances
   * { templateRef: string, name: string, displayName?: string, circle?: string,
   *   overrides?: object, lifecycleMode?: string, start?: boolean }
   */
  router.post('/instances', async (req, res) => {
    try {
      const { templateRef, name, displayName, circle, overrides, lifecycleMode, start } = req.body;

      // Support legacy field name
      const templateName = templateRef ?? req.body.templateName;

      if (!templateName || !name) {
        return res.status(400).json({ error: 'templateRef and name are required' });
      }

      // Verify template exists
      const template = await agentRegistry.getTemplate(templateName);
      if (!template) {
        return res.status(404).json({ error: `Template "${templateName}" not found` });
      }

      // Create instance in DB via registry (not AgentFactory, which may not have registry)
      const instance = await agentRegistry.createInstance({
        name,
        displayName,
        templateRef: templateName,
        circle,
        overrides,
        lifecycleMode,
      });

      // Optionally start the instance (spawn Docker container)
      if (start !== false) {
        try {
          await orchestrator.startInstance(instance.id);
        } catch (startErr) {
          // Instance created but failed to start — return it with error status
          logger.error(`Instance ${instance.id} created but failed to start:`, startErr);
        }
      }

      // Re-fetch to get updated status
      const updated = await agentRegistry.getInstance(instance.id);
      res.status(201).json(updated ?? instance);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Get agent instance detail ──────────────────────────────────────────────
  router.get('/instances/:id', async (req, res) => {
    try {
      const instance = await agentRegistry.getInstance(req.params.id);
      if (!instance) {
        return res.status(404).json({ error: `Agent instance "${req.params.id}" not found` });
      }
      res.json(instance);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Start an agent instance ────────────────────────────────────────────────
  router.post('/instances/:id/start', async (req, res) => {
    try {
      const { id } = req.params;
      const instance = await agentRegistry.getInstance(id);
      if (!instance) {
        return res.status(404).json({ error: `Agent instance "${id}" not found` });
      }

      await orchestrator.startInstance(id);
      const updated = await agentRegistry.getInstance(id);
      res.json(updated);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Stop an agent instance ─────────────────────────────────────────────────
  router.post('/instances/:id/stop', async (req, res) => {
    try {
      const { id } = req.params;
      const instance = await agentRegistry.getInstance(id);
      if (!instance) {
        return res.status(404).json({ error: `Agent instance "${id}" not found` });
      }

      await orchestrator.stopInstance(id);
      const updated = await agentRegistry.getInstance(id);
      res.json(updated);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Delete an agent instance ──────────────────────────────────────────────
  router.delete('/instances/:id', async (req, res) => {
    try {
      const { id } = req.params;
      if (!id) return res.status(400).json({ error: 'Instance ID is required' });

      // Stop the instance (cleans up Docker)
      await orchestrator.stopInstance(id);

      // Delete from DB
      await agentRegistry.deleteInstance(id);

      res.status(204).send();
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Get agent thoughts ───────────────────────────────────────────────────
  router.get('/instances/:id/thoughts', async (req, res) => {
    try {
      const { id } = req.params;
      const { taskId, limit, offset } = req.query;

      const intercom = orchestrator.getIntercom();
      if (!intercom) {
        return res.status(503).json({ error: 'Intercom service not available' });
      }

      const thoughts = await intercom.getThoughts(id, {
        taskId: taskId as string,
        limit: limit ? parseInt(limit as string) : 50,
        offset: offset ? parseInt(offset as string) : 0,
      });

      res.json(thoughts);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Legacy: get thoughts by name (redirects to instance ID lookup) ────────
  router.get('/:id/thoughts', async (req, res) => {
    try {
      const { id } = req.params;
      const { taskId, limit, offset } = req.query;

      const intercom = orchestrator.getIntercom();
      if (!intercom) {
        return res.status(503).json({ error: 'Intercom service not available' });
      }

      const thoughts = await intercom.getThoughts(id, {
        taskId: taskId as string,
        limit: limit ? parseInt(limit as string) : 50,
        offset: offset ? parseInt(offset as string) : 0,
      });

      res.json(thoughts);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Send a direct message to an agent instance (Story 9.3) ────────────────
  router.post('/:id/message', async (req, res) => {
    try {
      const { id } = req.params;
      const { from, payload } = req.body as { from?: string; payload?: Record<string, unknown> };

      if (!from || !payload) {
        return res.status(400).json({ error: 'Required fields: from, payload' });
      }

      const intercom = orchestrator.getIntercom();
      if (!intercom) {
        return res.status(503).json({ error: 'Intercom service not available' });
      }

      let fromManifest = orchestrator.getManifest(from);
      if (!fromManifest) {
        fromManifest = orchestrator.getManifestByInstanceId(from);
      }

      if (!fromManifest) {
        return res.status(404).json({ error: `Sender agent "${from}" not found` });
      }

      const msg = await intercom.sendDirectMessage(fromManifest, id, payload);
      res.json({ success: true, message: msg });
    } catch (err: unknown) {
      const error = err as Error;
      if (error.name === 'IntercomPermissionError') {
        return res.status(403).json({ error: error.message });
      }
      res.status(500).json({ error: error.message });
    }
  });

  // ── POST /api/agents/validate ────────────────────────────────────────────
  router.post('/validate', (req, res) => {
    const body = req.body;
    if (!body || typeof body !== 'object') {
      return res.json({ valid: false, errors: ['Request body must be a JSON object'] });
    }
    try {
      AgentManifestLoader.validateManifest(body, 'POST /api/agents/validate');
      res.json({ valid: true });
    } catch (err: unknown) {
      const error = err as Error;
      res.json({ valid: false, errors: [error.message] });
    }
  });

  // ── POST /api/agents/test-chat ───────────────────────────────────────────
  router.post('/test-chat', async (req, res) => {
    try {
      const { manifest, message, history = [] } = req.body;

      if (!manifest || !message) {
        return res.status(400).json({ error: 'manifest and message are required' });
      }

      AgentManifestLoader.validateManifest(manifest, 'POST /api/agents/test-chat');
      const agent = AgentFactory.createAgent(manifest);

      const toolExecutor = orchestrator.getToolExecutor();
      if (toolExecutor) {
        agent.setToolExecutor(toolExecutor);
      }

      const response = await agent.process(message, history);

      res.json({
        reply: response.finalAnswer || response.thought || 'No response.',
        thought: response.thought,
      });
    } catch (err: unknown) {
      const error = err as Error;
      logger.error('Preview chat error:', error);
      res.status(500).json({ error: error.message });
    }
  });

  return router;
}
