/**
 * Skill Search Handler — reads /sera/skills/index.json and returns matching skills.
 *
 * Part of the Universal Skill Registry (M6.2). The index.json file is generated
 * at container spawn time by ManifestSkillIndex.generateIndex() and contains only
 * the skills the agent is authorized to use (capability-filtered).
 */

import fs from 'fs';
import { log } from '../logger.js';

const SKILLS_INDEX_PATH = process.env['SERA_SKILLS_DIR']
  ? `${process.env['SERA_SKILLS_DIR']}/index.json`
  : '/sera/skills/index.json';

interface SkillEntry {
  name: string;
  displayName: string;
  description: string;
  version: string;
  parameters: Record<string, unknown>;
  returns?: Record<string, unknown>;
  compatibleHarnesses?: string[];
  tags?: string[];
}

interface SkillIndex {
  version: string;
  generatedAt: string;
  skills: SkillEntry[];
}

let cachedIndex: SkillIndex | null = null;

function loadIndex(): SkillIndex | null {
  if (cachedIndex) return cachedIndex;
  try {
    if (!fs.existsSync(SKILLS_INDEX_PATH)) {
      log('debug', `No skill index found at ${SKILLS_INDEX_PATH}`);
      return null;
    }
    const raw = fs.readFileSync(SKILLS_INDEX_PATH, 'utf-8');
    cachedIndex = JSON.parse(raw) as SkillIndex;
    return cachedIndex;
  } catch (err) {
    log('warn', `Failed to load skill index: ${err instanceof Error ? err.message : String(err)}`);
    return null;
  }
}

/**
 * Search the mounted skill index for skills matching a query.
 */
export async function skillSearch(args: Record<string, unknown>): Promise<string> {
  const query = ((args['query'] as string) ?? '').toLowerCase();
  const harness = args['harness'] as string | undefined;
  const tag = args['tag'] as string | undefined;

  const index = loadIndex();
  if (!index) {
    return JSON.stringify({
      error: 'No skill index available. This agent may not have skill packages mounted.',
      skills: [],
    });
  }

  const results = index.skills.filter((skill) => {
    // Text match
    const textMatch =
      !query ||
      skill.name.toLowerCase().includes(query) ||
      skill.displayName.toLowerCase().includes(query) ||
      skill.description.toLowerCase().includes(query) ||
      (skill.tags ?? []).some((t) => t.toLowerCase().includes(query));

    // Harness filter
    const harnessMatch = !harness || (skill.compatibleHarnesses ?? []).includes(harness);

    // Tag filter
    const tagMatch = !tag || (skill.tags ?? []).includes(tag);

    return textMatch && harnessMatch && tagMatch;
  });

  return JSON.stringify({
    query,
    total: results.length,
    skills: results.map((s) => ({
      name: s.name,
      displayName: s.displayName,
      description: s.description,
      version: s.version,
      compatibleHarnesses: s.compatibleHarnesses,
      tags: s.tags,
    })),
  });
}
