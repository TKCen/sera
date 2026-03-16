/**
 * Agent Management Routes
 *
 * CRUD operations for agent manifests + live reload support.
 */

import { Router } from 'express';
import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { AgentManifestLoader } from '../agents/manifest/AgentManifestLoader.js';

export function createAgentRouter(
  orchestrator: Orchestrator,
  agentsDir: string,
) {
  const router = Router();

  // ── List all agents ────────────────────────────────────────────────────────
  /**
   * Lists all loaded agents.
   * @param req Express request
   * @param res Express response
   * @returns {void}
   */
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
    const info = orchestrator.getAgentInfo(req.params.name);
    if (!info) {
      return res.status(404).json({ error: `Agent "${req.params.name}" not found` });
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
    const name = req.params.name;
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
   * Updates the agent's manifest and triggers a live reload.
   * @param req Express request containing agent name in params and updated manifest in body
   * @param res Express response
   * @returns {void}
   */
  router.put('/:name/manifest', (req, res) => {
    const name = req.params.name;
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
      const filePath = findManifestFile(agentsDir, name) ?? path.join(agentsDir, `${name}.agent.yaml`);
      fs.writeFileSync(filePath, yamlStr, 'utf-8');

      // Trigger live reload
      const result = orchestrator.reloadAgents();

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
      const result = orchestrator.reloadAgents();
      res.json({ success: true, ...result });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Find the YAML manifest file for a given agent name.
 * Searches for `<name>.agent.yaml` in the agents directory.
 */
function findManifestFile(agentsDir: string, agentName: string): string | undefined {
  if (!fs.existsSync(agentsDir)) return undefined;

  const files = fs.readdirSync(agentsDir).filter(f => f.endsWith('.agent.yaml'));
  for (const file of files) {
    try {
      const filePath = path.join(agentsDir, file);
      const manifest = AgentManifestLoader.loadManifest(filePath);
      if (manifest.metadata.name === agentName) {
        return filePath;
      }
    } catch {
      // Skip invalid manifests
    }
  }

  return undefined;
}
