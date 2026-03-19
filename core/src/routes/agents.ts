/**
 * Agent Management Routes
 *
 * CRUD operations for agent manifests + live reload support.
 */

import { Router } from 'express';
import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import { v4 as uuidv4 } from 'uuid';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { AgentManifestLoader } from '../agents/manifest/AgentManifestLoader.js';
import { AgentFactory } from '../agents/AgentFactory.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('AgentRouter');

export function createAgentRouter(
  orchestrator: Orchestrator,
  agentsDir: string,
) {
  const router = Router();

  // ── List all agent templates ───────────────────────────────────────────────
  /**
   * Lists all loaded agent manifests (templates).
   */
  router.get('/templates', (req, res) => {
    res.json(orchestrator.listAgents());
  });

  // ── List all agent instances ───────────────────────────────────────────────
  /**
   * Lists all persistent agent instances from the database.
   */
  router.get('/instances', async (req, res) => {
    try {
      const templateName = req.query.template as string | undefined;
      const instances = await AgentFactory.listInstances(templateName);
      res.json(instances);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── Create a new agent instance ────────────────────────────────────────────
  /**
   * Creates a new persistent agent instance from a template and starts it.
   * POST /api/agents/instances { templateName: string, name: string, workspacePath?: string }
   */
  router.post('/instances', async (req, res) => {
    try {
      const { templateName, name, workspacePath } = req.body;
      if (!templateName || !name) {
        return res.status(400).json({ error: 'templateName and name are required' });
      }

      // 1. Create instance in DB
      const instance = await AgentFactory.createInstance(templateName, name, workspacePath);

      // 2. Start it in Orchestrator (this will handle Docker instantiation if configured)
      await orchestrator.startInstance(instance.id);

      res.status(201).json(instance);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── Get agent instance detail ──────────────────────────────────────────────
  /**
   * Gets detailed information for a specific agent instance.
   */
  router.get('/instances/:id', async (req, res) => {
    try {
      const instance = await AgentFactory.getInstance(req.params.id);
      if (!instance) {
        return res.status(404).json({ error: `Agent instance "${req.params.id}" not found` });
      }
      res.json(instance);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });
  
  // ── Get agent thoughts ───────────────────────────────────────────────────
  /**
   * Gets persisted thoughts for a specific agent instance.
   * Story 9.7 persistence.
   */
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
        offset: offset ? parseInt(offset as string) : 0
      });
      
      res.json(thoughts);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── Delete an agent instance ──────────────────────────────────────────────
  /**
   * Stops the agent container and deletes the instance from the database.
   * DELETE /api/agents/instances/:id
   */
  router.delete('/instances/:id', async (req, res) => {
    try {
      const { id } = req.params;
      if (!id) return res.status(400).json({ error: 'Instance ID is required' });

      // 1. Stop the instance (cleans up Docker)
      await orchestrator.stopInstance(id);

      // 2. Delete from DB
      await AgentFactory.deleteInstance(id);

      res.status(204).send();
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── POST /api/agents/test-chat ───────────────────────────────────────────
  /**
   * Temporary "preview" chat with a non-persisted manifest.
   */
  router.post('/test-chat', async (req, res) => {
    try {
      const { manifest, message, history = [] } = req.body;

      if (!manifest || !message) {
        return res.status(400).json({ error: 'manifest and message are required' });
      }

      // Validate manifest
      AgentManifestLoader.validateManifest(manifest, 'POST /api/agents/test-chat');

      // Create a transient agent instance (not registered in Orchestrator's main map)
      const agent = AgentFactory.createAgent(manifest);

      // If orchestrator has a tool executor, attach it
      const toolExecutor = orchestrator.getToolExecutor();
      if (toolExecutor) {
        agent.setToolExecutor(toolExecutor);
      }

      const response = await agent.process(message, history);

      res.json({
        reply: response.finalAnswer || response.thought || 'No response.',
        thought: response.thought
      });
    } catch (err: any) {
      logger.error('Preview chat error:', err);
      res.status(500).json({ error: err.message });
    }
  });

  // ── Legacy / Compatibility ──────────────────────────────────────────────────
  router.get('/', (req, res) => {
    res.json(orchestrator.listAgents());
  });

  // ── Get agent detail ───────────────────────────────────────────────────────
  /**
   * Gets detailed information for a specific agent.
   * @param req Express request containing agent name in params
   * @param res Express response
   * @returns {void}
   */
  router.get('/:name', (req, res) => {
    const name = sanitizeAgentName(req.params.name);
    const info = orchestrator.getAgentInfo(name);
    if (!info) {
      return res.status(404).json({ error: `Agent "${name}" not found` });
    }
    res.json(info);
  });

  // ── Get raw YAML manifest ─────────────────────────────────────────────────
  /**
   * Retrieves the raw YAML manifest file for the agent.
   * @param req Express request containing agent name in params
   * @param res Express response
   * @returns {void}
   */
  router.get('/:name/manifest/raw', (req, res) => {
    const name = sanitizeAgentName(req.params.name);
    const filePath = findManifestFile(agentsDir, name);
    if (!filePath) {
      return res.status(404).json({ error: `Manifest file for "${name}" not found` });
    }

    try {
      const raw = fs.readFileSync(filePath, 'utf-8');
      res.type('text/yaml').send(raw);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── Update agent manifest ─────────────────────────────────────────────────
  /**
   * Updates the agent's manifest (or creates it) and triggers a live reload.
   * @param req Express request containing agent name in params and updated manifest in body
   * @param res Express response
   * @returns {void}
   */
  router.put('/:name/manifest', (req, res) => {
    const name = sanitizeAgentName(req.params.name);
    const body = req.body;

    if (!body || typeof body !== 'object') {
      return res.status(400).json({ error: 'Request body must be a JSON manifest object' });
    }

    try {
      // Validate the manifest before writing
      AgentManifestLoader.validateManifest(body, `PUT /api/agents/${name}/manifest`);

      // Ensure metadata.name matches the URL param
      if (body.metadata?.name !== name) {
        return res.status(400).json({
          error: `Manifest metadata.name "${body.metadata?.name}" does not match URL parameter "${name}"`,
        });
      }

      // Serialize to YAML and write
      const yamlStr = yaml.dump(body, { lineWidth: 120, noRefs: true, sortKeys: false });
      let filePath = findManifestFile(agentsDir, name);

      if (!filePath) {
        // Create new directory for agent if it doesn't exist
        const agentDir = path.join(agentsDir, name);
        if (!fs.existsSync(agentDir)) {
          fs.mkdirSync(agentDir, { recursive: true });
        }
        filePath = path.join(agentDir, 'AGENT.yaml');
      }

      fs.writeFileSync(filePath, yamlStr, 'utf-8');

      // Trigger live reload
      const result = orchestrator.reloadTemplates();

      res.json({ success: true, ...result });
    } catch (err: any) {
      if (err.name === 'ManifestValidationError') {
        return res.status(400).json({ error: err.message, field: err.field });
      }
      res.status(500).json({ error: err.message });
    }
  });

  // ── Force reload all manifests ────────────────────────────────────────────
  /**
   * Forces a full reload of all agent manifests from disk.
   * @param req Express request
   * @param res Express response
   * @returns {void}
   */
  router.post('/reload', (req, res) => {
    try {
      const result = orchestrator.reloadTemplates();
      res.json({ success: true, ...result });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Basic agent name sanitization to prevent directory traversal.
 */
function sanitizeAgentName(name: string): string {
  return name.replace(/[..\\/]/g, '').toLowerCase();
}

/**
 * Find the YAML manifest file for a given agent name.
 * Searches for `<name>.agent.yaml` in the agents directory and AGENT.yaml in subdirectories.
 */
function findManifestFile(agentsDir: string, agentName: string): string | undefined {
  if (!fs.existsSync(agentsDir)) return undefined;

  const entries = fs.readdirSync(agentsDir, { withFileTypes: true });
  for (const entry of entries) {
    let filePath: string | undefined;

    if (entry.isFile() && entry.name.endsWith('.agent.yaml')) {
      filePath = path.join(agentsDir, entry.name);
    } else if (entry.isDirectory()) {
      const subDirAgentFile = path.join(agentsDir, entry.name, 'AGENT.yaml');
      if (fs.existsSync(subDirAgentFile)) {
        filePath = subDirAgentFile;
      }
    }

    if (filePath) {
      try {
        const manifest = AgentManifestLoader.loadManifest(filePath);
        if (manifest.metadata.name === agentName) {
          return filePath;
        }
      } catch {
        // Skip invalid manifests
      }
    }
  }

  return undefined;
}
