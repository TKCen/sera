/**
 * Circle Management Routes
 *
 * CRUD operations for circle manifests + project context editing.
 */

import { Router } from 'express';
import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import type { CircleRegistry } from '../circles/CircleRegistry.js';
import type { AgentManifest } from '../agents/manifest/types.js';
import type { Orchestrator } from '../agents/Orchestrator.js';

export function createCircleRouter(
  circleRegistry: CircleRegistry,
  circlesDir: string,
  getAgentManifests: () => AgentManifest[],
  orchestrator: Orchestrator,
) {
  const router = Router();

  // ── List all circles ──────────────────────────────────────────────────────
  /**
   * Returns summaries of all registered circles.
   * @param req Express request
   * @param res Express response
   * @returns {void}
   */
  router.get('/', (req, res) => {
    res.json(circleRegistry.listCircleSummaries());
  });

  // ── Get circle detail ─────────────────────────────────────────────────────
  /**
   * Retrieves details and project context for a specific circle.
   * @param req Express request containing circle name in params
   * @param res Express response
   * @returns {void}
   */
  router.get('/:name', (req, res) => {
    const circle = circleRegistry.getCircle(req.params.name);
    if (!circle) {
      return res.status(404).json({ error: `Circle "${req.params.name}" not found` });
    }
    res.json({
      ...circle,
      projectContext: circleRegistry.getProjectContext(req.params.name) ?? null,
    });
  });

  // ── Create new circle ─────────────────────────────────────────────────────
  /**
   * Creates a new circle and saves its manifest.
   * @param req Express request containing new circle manifest in body
   * @param res Express response
   * @returns {void}
   */
  router.post('/', (req, res) => {
    try {
      const body = req.body;
      if (!body || typeof body !== 'object') {
        return res.status(400).json({ error: 'Request body must be a JSON circle manifest object' });
      }

      // Ensure required fields
      if (!body.metadata?.name) {
        return res.status(400).json({ error: 'metadata.name is required' });
      }

      // Check for duplicate
      if (circleRegistry.getCircle(body.metadata.name)) {
        return res.status(409).json({ error: `Circle "${body.metadata.name}" already exists` });
      }

      // Defaults
      body.apiVersion = body.apiVersion || 'sera/v1';
      body.kind = 'Circle';
      body.agents = body.agents || [];

      // Write to YAML
      const yamlStr = yaml.dump(body, { lineWidth: 120, noRefs: true, sortKeys: false });
      const filePath = path.join(circlesDir, `${body.metadata.name}.circle.yaml`);
      fs.writeFileSync(filePath, yamlStr, 'utf-8');

      // Reload circles
      circleRegistry.loadFromDirectory(circlesDir, getAgentManifests()).then(() => {
        res.status(201).json({ success: true, name: body.metadata.name });
      }).catch(err => res.status(500).json({ error: err.message }));

    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── Update circle manifest ────────────────────────────────────────────────
  /**
   * Updates an existing circle's configuration.
   * @param req Express request containing circle name in params and updated manifest in body
   * @param res Express response
   * @returns {void}
   */
  router.put('/:name', (req, res) => {
    const name = req.params.name;
    const circle = circleRegistry.getCircle(name);
    if (!circle) {
      return res.status(404).json({ error: `Circle "${name}" not found` });
    }

    try {
      const body = req.body;
      body.apiVersion = body.apiVersion || circle.apiVersion;
      body.kind = 'Circle';

      const yamlStr = yaml.dump(body, { lineWidth: 120, noRefs: true, sortKeys: false });
      const filePath = findCircleFile(circlesDir, name) ?? path.join(circlesDir, `${name}.circle.yaml`);
      fs.writeFileSync(filePath, yamlStr, 'utf-8');

      // Reload
      circleRegistry.loadFromDirectory(circlesDir, getAgentManifests()).then(() => {
        res.json({ success: true });
      }).catch(err => res.status(500).json({ error: err.message }));

    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── Delete circle ─────────────────────────────────────────────────────────
  /**
   * Removes a circle manifest from disk.
   * @param req Express request containing circle name in params
   * @param res Express response
   * @returns {void}
   */
  router.delete('/:name', (req, res) => {
    const name = req.params.name;
    const filePath = findCircleFile(circlesDir, name);
    if (!filePath) {
      return res.status(404).json({ error: `Circle "${name}" not found on disk` });
    }

    try {
      fs.unlinkSync(filePath);
      // Reload to remove from registry
      circleRegistry.loadFromDirectory(circlesDir, getAgentManifests()).then(() => {
        res.json({ success: true });
      }).catch(err => res.status(500).json({ error: err.message }));
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── Update project context ────────────────────────────────────────────────
  /**
   * Updates the project context markdown file for a circle.
   * @param req Express request containing circle name in params and context content in body
   * @param res Express response
   * @returns {void}
   */
  router.put('/:name/context', (req, res) => {
    const name = req.params.name;
    const circle = circleRegistry.getCircle(name);
    if (!circle) {
      return res.status(404).json({ error: `Circle "${name}" not found` });
    }

    const { content } = req.body;
    if (typeof content !== 'string') {
      return res.status(400).json({ error: 'content (string) is required' });
    }

    try {
      // Determine the context file path
      const contextPath = circle.projectContext?.path
        ? path.resolve(circlesDir, circle.projectContext.path)
        : path.join(circlesDir, name, 'project-context.md');

      // Ensure directory exists
      const dir = path.dirname(contextPath);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }

      fs.writeFileSync(contextPath, content, 'utf-8');

      // Reload project context
      circleRegistry.loadProjectContext(circle, circlesDir);

      res.json({ success: true });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── Party Mode routes (moved from index.ts) ───────────────────────────────
  // Note: Party mode routes stay in index.ts for now since they need the
  // PartySessionManager instance. We'll move them in a future refactor.

  return router;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function findCircleFile(circlesDir: string, circleName: string): string | undefined {
  if (!fs.existsSync(circlesDir)) return undefined;
  const files = fs.readdirSync(circlesDir).filter(f => f.endsWith('.circle.yaml'));
  for (const file of files) {
    try {
      const raw = yaml.load(fs.readFileSync(path.join(circlesDir, file), 'utf-8')) as any;
      if (raw?.metadata?.name === circleName) {
        return path.join(circlesDir, file);
      }
    } catch {
      // Skip invalid files
    }
  }
  return undefined;
}
