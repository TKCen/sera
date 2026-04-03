/**
 * ManifestSkillIndex — indexes sera-skill.json manifests from a directory.
 *
 * This is a discovery-only index for the Universal Skill Registry (M6).
 * It is separate from SkillRegistry (executable tools) and SkillLibrary
 * (DB-backed guidance). Skills indexed here may or may not be executable
 * by the current harness — the compatibleHarnesses field indicates
 * which harnesses can run them.
 */

import fs from 'fs';
import path from 'path';
import { Logger } from '../lib/logger.js';

const logger = new Logger('ManifestSkillIndex');

export interface ManifestSkill {
  name: string;
  displayName: string;
  description: string;
  version: string;
  parameters: Record<string, unknown>;
  returns?: Record<string, unknown>;
  compatibleHarnesses?: string[];
  tags?: string[];
  author?: string;
  license?: string;
}

export class ManifestSkillIndex {
  private skills: Map<string, ManifestSkill> = new Map();
  private static instance: ManifestSkillIndex | undefined;

  static getInstance(): ManifestSkillIndex {
    if (!ManifestSkillIndex.instance) {
      ManifestSkillIndex.instance = new ManifestSkillIndex();
    }
    return ManifestSkillIndex.instance;
  }

  /** Scan a directory for sera-skill.json files and index them. */
  loadFromDirectory(dir: string): void {
    if (!fs.existsSync(dir)) {
      logger.debug(`Skills directory ${dir} does not exist — no manifest skills loaded`);
      return;
    }

    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const manifestPath = path.join(dir, entry.name, 'sera-skill.json');
      if (!fs.existsSync(manifestPath)) continue;

      try {
        const raw = fs.readFileSync(manifestPath, 'utf-8');
        const manifest = JSON.parse(raw) as ManifestSkill;
        if (!manifest.name || !manifest.description || !manifest.version) {
          logger.warn(`Invalid manifest in ${manifestPath}: missing required fields`);
          continue;
        }
        this.skills.set(manifest.name, manifest);
        logger.debug(`Indexed manifest skill: ${manifest.name} v${manifest.version}`);
      } catch (err) {
        logger.warn(
          `Failed to load ${manifestPath}: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }

    logger.info(`Loaded ${this.skills.size} manifest skills from ${dir}`);
  }

  /** Return all indexed skills. */
  listAll(): ManifestSkill[] {
    return [...this.skills.values()];
  }

  /** Search skills by query string and optional filters. */
  search(query: string, filters?: { harness?: string; tags?: string[] }): ManifestSkill[] {
    const q = query.toLowerCase();
    return this.listAll().filter((skill) => {
      // Text match on name, displayName, description
      const textMatch =
        !q ||
        skill.name.toLowerCase().includes(q) ||
        skill.displayName.toLowerCase().includes(q) ||
        skill.description.toLowerCase().includes(q) ||
        (skill.tags ?? []).some((t) => t.toLowerCase().includes(q));

      // Harness filter
      const harnessMatch =
        !filters?.harness || (skill.compatibleHarnesses ?? []).includes(filters.harness);

      // Tags filter
      const tagsMatch =
        !filters?.tags?.length || filters.tags.every((t) => (skill.tags ?? []).includes(t));

      return textMatch && harnessMatch && tagsMatch;
    });
  }

  /** Get a single skill by name. */
  get(name: string): ManifestSkill | undefined {
    return this.skills.get(name);
  }

  /** Number of indexed skills. */
  get size(): number {
    return this.skills.size;
  }

  /** Generate an index.json file for container mounting. */
  generateIndex(outputPath: string, skillNames?: string[]): void {
    const skills = skillNames
      ? this.listAll().filter((s) => skillNames.includes(s.name))
      : this.listAll();

    const index = {
      version: '1.0',
      generatedAt: new Date().toISOString(),
      skills: skills.map((s) => ({
        name: s.name,
        displayName: s.displayName,
        description: s.description,
        version: s.version,
        parameters: s.parameters,
        returns: s.returns,
        compatibleHarnesses: s.compatibleHarnesses,
        tags: s.tags,
      })),
    };

    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    fs.writeFileSync(outputPath, JSON.stringify(index, null, 2), 'utf-8');
  }
}
