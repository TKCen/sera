/**
 * Skills & Tools Routes
 *
 * Listing and management of registered skills.
 */

import { Router } from 'express';
import type { SkillRegistry } from '../skills/SkillRegistry.js';
import type { Orchestrator } from '../agents/Orchestrator.js';

export function createSkillsRouter(
  skillRegistry: SkillRegistry,
  orchestrator: Orchestrator,
) {
  const router = Router();

  // ── List all skills ───────────────────────────────────────────────────────
  router.get('/', (req, res) => {
    const skills = skillRegistry.listAll();

    // Enrich with usage info: which agents reference each skill
    const manifests = orchestrator.getAllManifests();
    const enriched = skills.map(skill => {
      const usedBy: string[] = [];
      for (const manifest of manifests) {
        const agentSkills = new Set<string>([
          ...(manifest.skills ?? []),
          ...(manifest.tools?.allowed ?? []),
        ]);
        if (agentSkills.has(skill.id)) {
          usedBy.push(manifest.metadata.name);
        }
      }
      return { ...skill, usedBy };
    });

    res.json(enriched);
  });

  // ── Update an agent's tools.allowed list ──────────────────────────────────
  router.put('/agents/:name/tools', (req, res) => {
    const name = req.params.name;
    const manifest = orchestrator.getManifest(name);
    if (!manifest) {
      return res.status(404).json({ error: `Agent "${name}" not found` });
    }

    const { allowed } = req.body;
    if (!Array.isArray(allowed)) {
      return res.status(400).json({ error: 'allowed must be an array of skill/tool IDs' });
    }

    // This is informational only — the actual persistence is handled
    // via the agent manifest update endpoint PUT /api/agents/:name/manifest
    res.json({
      success: true,
      message: 'Use PUT /api/agents/:name/manifest to persist tool changes',
      currentAllowed: manifest.tools?.allowed ?? [],
      requested: allowed,
    });
  });

  return router;
}
