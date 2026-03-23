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
import { SkillRegistryService } from '../skills/adapters/SkillRegistryService.js';

export function createSkillsRouter(
  _skillRegistry: SkillRegistry,
  orchestrator: Orchestrator,
  pool: Pool
) {
  const router = Router();

  // ── List guidance skills (text documents, not executable tools) ───────────
  /**
   * Returns only guidance skills from the SkillLibrary (markdown documents
   * stored in the DB). Executable tools are served by GET /api/tools.
   */
  router.get('/', async (_req, res) => {
    try {
      const skillLibrary = SkillLibrary.getInstance(pool);
      const guidanceSkills = await skillLibrary.listSkills();

      // Enrich with usage info
      const manifests = orchestrator.getAllManifests();
      const enriched = guidanceSkills.map((skill) => {
        const usedBy: string[] = [];
        for (const manifest of manifests) {
          const skills = (manifest.skills ?? []).map((s) => (typeof s === 'string' ? s : s.name));
          if (skills.includes(skill.name)) {
            usedBy.push(manifest.metadata.name);
          }
        }
        return { ...skill, id: skill.name, usedBy };
      });

      res.json(enriched);
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Search external skill registries (must be before /:name) ─────────────
  router.get('/registry/search', async (req, res) => {
    try {
      const query = (req.query.q as string) || '';
      const source = req.query.source as string | undefined;

      const registryService = SkillRegistryService.getInstance(pool);
      const results = await registryService.search(query, source);

      res.json(results);
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

  // ── Create or update a guidance skill ────────────────────────────────────
  router.post('/', async (req, res) => {
    try {
      const { name, version, description, triggers, category, tags, maxTokens, content } =
        req.body as {
          name?: string;
          version?: string;
          description?: string;
          triggers?: string[];
          category?: string;
          tags?: string[];
          maxTokens?: number;
          content?: string;
        };

      if (!name || !version || !description || !content) {
        return res
          .status(400)
          .json({ error: 'name, version, description, and content are required' });
      }

      const skillLibrary = SkillLibrary.getInstance(pool);
      await skillLibrary.createSkill(
        {
          name,
          version,
          description,
          triggers: triggers ?? [],
          ...(category ? { category } : {}),
          ...(tags ? { tags } : {}),
          ...(maxTokens != null ? { maxTokens } : {}),
        },
        content
      );

      res.status(201).json({ message: `Skill "${name}" v${version} created` });
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Delete a guidance skill ────────────────────────────────────────────────
  router.delete('/:name', async (req, res) => {
    try {
      const { name } = req.params;
      const { version } = req.query;

      const skillLibrary = SkillLibrary.getInstance(pool);
      const deleted = await skillLibrary.deleteSkill(name, version as string | undefined);

      if (!deleted) {
        return res.status(404).json({ error: `Skill "${name}" not found` });
      }

      res.json({ message: `Skill "${name}" deleted` });
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Import a skill from an external registry ──────────────────────────────
  router.post('/import', async (req, res) => {
    try {
      const { source, skillId } = req.body as { source?: string; skillId?: string };

      if (!source || !skillId) {
        return res.status(400).json({ error: 'source and skillId are required' });
      }

      const registryService = SkillRegistryService.getInstance(pool);
      await registryService.importSkill(source, skillId);

      res.status(201).json({ message: `Skill "${skillId}" imported from ${source}` });
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  // ── Update an agent's tools.allowed list ──────────────────────────────────
  router.put('/agents/:name/tools', async (req, res) => {
    const name = req.params['name']!;

    const { allowed } = req.body as { allowed?: unknown };
    if (!Array.isArray(allowed) || !allowed.every((a) => typeof a === 'string')) {
      return res.status(400).json({ error: 'allowed must be an array of skill/tool IDs' });
    }

    try {
      // Find instance by name
      const { rows } = await pool.query<{ id: string; overrides: Record<string, unknown> }>(
        'SELECT id, overrides FROM agent_instances WHERE name = $1',
        [name]
      );
      if (rows.length === 0) {
        return res.status(404).json({ error: `Agent instance "${name}" not found` });
      }

      const instance = rows[0]!;
      const overrides = { ...instance.overrides, tools: { allowed } };

      // Persist to database
      await pool.query(
        'UPDATE agent_instances SET overrides = $1, updated_at = NOW() WHERE id = $2',
        [JSON.stringify(overrides), instance.id]
      );

      // Update in-memory manifest if loaded
      const manifest = orchestrator.getManifest(name);
      if (manifest) {
        if (!manifest.tools) {
          manifest.tools = { allowed };
        } else {
          manifest.tools.allowed = allowed as string[];
        }
      }

      res.json({ success: true, allowed });
    } catch (err) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  return router;
}
