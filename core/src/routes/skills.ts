/**
 * Skills & Tools Routes
 *
 * Listing and management of registered skills.
 */

import { Router } from 'express';
import type { Pool } from 'pg';
import type { SkillRegistry } from '../skills/SkillRegistry.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { SkillLibrary } from '../skills/SkillLibrary.js';

export function createSkillsRouter(
  skillRegistry: SkillRegistry,
  orchestrator: Orchestrator,
  pool: Pool
) {
  const router = Router();

  // ── List all skills ───────────────────────────────────────────────────────
  /**
   * Lists all registered skills (both executable and guidance docs).
   */
  router.get('/', async (req, res) => {
    try {
      // 1. Get executable skills (builtin + MCP) from registry
      const executableSkills = skillRegistry.listAll();

      // 2. Get guidance skills from SkillLibrary (DB)
      const skillLibrary = SkillLibrary.getInstance(pool);
      const guidanceSkills = await skillLibrary.listSkills();

      // Enrich executable skills with usage info
      const manifests = orchestrator.getAllManifests();
      const enrichedExec = executableSkills.map((skill) => {
        const usedBy: string[] = [];
        for (const manifest of manifests) {
          const skills = (manifest.skills ?? []).map((s) => (typeof s === 'string' ? s : s.name));
          const agentSkills = new Set<string>([...skills, ...(manifest.tools?.allowed ?? [])]);
          if (agentSkills.has(skill.id)) {
            usedBy.push(manifest.metadata.name);
          }
        }
        return { ...skill, type: 'executable', usedBy };
      });

      // Enrich guidance skills with usage info
      const enrichedGuidance = guidanceSkills.map((skill) => {
        const usedBy: string[] = [];
        for (const manifest of manifests) {
          const skills = (manifest.skills ?? []).map((s) => (typeof s === 'string' ? s : s.name));
          if (skills.includes(skill.name)) {
            usedBy.push(manifest.metadata.name);
          }
        }
        return { ...skill, id: skill.name, type: 'guidance', usedBy };
      });

      res.json([...enrichedExec, ...enrichedGuidance]);
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Get a specific guidance skill ─────────────────────────────────────────
  router.get('/:name', async (req, res) => {
    try {
      const { name } = req.params;
      const { version } = req.query;

      const skillLibrary = SkillLibrary.getInstance(pool);
      const skill = await skillLibrary.getSkill(name, version as string);

      if (!skill) {
        return res.status(404).json({ error: `Skill "${name}" not found` });
      }

      res.json(skill);
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
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

    res.json({
      success: true,
      message: 'Use PUT /api/agents/:name/manifest to persist tool changes',
      currentAllowed: manifest.tools?.allowed ?? [],
      requested: allowed,
    });
  });

  return router;
}
