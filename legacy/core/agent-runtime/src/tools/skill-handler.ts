/**
 * Skill Handler — reads /sera/skills/index.json for skill discovery and
 * loads full skill content from skill subdirectories on demand.
 *
 * Implements progressive disclosure (M6.2):
 *   Tier 1 (metadata)  — name + description injected into system prompt at startup
 *   Tier 2 (content)   — full skill content loaded via view_skill tool
 *   Tier 3 (references) — supporting files loaded on demand
 */

import fs from 'fs';
import path from 'path';
import { log } from '../logger.js';

const SKILLS_DIR = process.env['SERA_SKILLS_DIR'] ?? '/sera/skills';
const SKILLS_INDEX_PATH = `${SKILLS_DIR}/index.json`;

/** Maximum length for a skill name in Tier 1 metadata. */
const TIER1_NAME_MAX = 64;
/** Maximum length for a skill description in Tier 1 metadata. */
const TIER1_DESC_MAX = 1024;

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
 * Build a compact Tier 1 metadata block suitable for injection into the system
 * prompt. Only name (≤64 chars) and description (≤1024 chars) are included.
 *
 * Returns an empty string when no skill index is available.
 */
export function buildSkillsMetadataBlock(): string {
  const index = loadIndex();
  if (!index || index.skills.length === 0) return '';

  const lines = ['Available skills (use view_skill to see full details):'];
  for (const skill of index.skills) {
    const name = skill.name.slice(0, TIER1_NAME_MAX);
    const desc = skill.description.slice(0, TIER1_DESC_MAX);
    lines.push(`- ${name}: ${desc}`);
  }
  return lines.join('\n');
}

/**
 * list_skills tool — returns Tier 1 metadata for all available skills.
 */
export async function listSkills(): Promise<string> {
  const index = loadIndex();
  if (!index) {
    return JSON.stringify({
      error: 'No skill index available. This agent may not have skill packages mounted.',
      skills: [],
    });
  }

  return JSON.stringify({
    total: index.skills.length,
    skills: index.skills.map((s) => ({
      name: s.name.slice(0, TIER1_NAME_MAX),
      description: s.description.slice(0, TIER1_DESC_MAX),
    })),
  });
}

/**
 * view_skill tool — returns Tier 2 (full content) and optionally Tier 3
 * (references) for a named skill.
 *
 * Full content is read from {SKILLS_DIR}/{skill_name}/content.md or
 * README.md. References are loaded from {SKILLS_DIR}/{skill_name}/refs/.
 */
export async function viewSkill(args: Record<string, unknown>): Promise<string> {
  const name = args['name'] as string | undefined;
  const includeRefs = Boolean(args['include_references']);

  if (!name || typeof name !== 'string') {
    return JSON.stringify({ error: 'Missing required parameter: name' });
  }

  const index = loadIndex();
  if (!index) {
    return JSON.stringify({ error: 'No skill index available.' });
  }

  const entry = index.skills.find((s) => s.name === name);
  if (!entry) {
    return JSON.stringify({
      error: `Skill "${name}" not found. Use list_skills to see available skills.`,
    });
  }

  // Tier 2: full content from file
  const skillDir = path.join(SKILLS_DIR, name);
  let content: string | null = null;

  for (const candidate of ['content.md', 'README.md', 'index.md']) {
    const fullPath = path.join(skillDir, candidate);
    if (fs.existsSync(fullPath)) {
      try {
        content = fs.readFileSync(fullPath, 'utf-8');
        break;
      } catch (err) {
        log(
          'warn',
          `Failed to read skill content ${fullPath}: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }
  }

  const result: Record<string, unknown> = {
    name: entry.name,
    displayName: entry.displayName,
    description: entry.description,
    version: entry.version,
    parameters: entry.parameters,
    ...(entry.returns !== undefined ? { returns: entry.returns } : {}),
    ...(entry.compatibleHarnesses !== undefined
      ? { compatibleHarnesses: entry.compatibleHarnesses }
      : {}),
    ...(entry.tags !== undefined ? { tags: entry.tags } : {}),
    content: content ?? '(No content file found for this skill.)',
  };

  // Tier 3: reference files from refs/ subdirectory
  if (includeRefs) {
    const refsDir = path.join(skillDir, 'refs');
    const references: Array<{ file: string; content: string }> = [];

    if (fs.existsSync(refsDir)) {
      try {
        const refFiles = fs.readdirSync(refsDir);
        for (const file of refFiles) {
          const refPath = path.join(refsDir, file);
          try {
            const refContent = fs.readFileSync(refPath, 'utf-8');
            references.push({ file, content: refContent });
          } catch (err) {
            log(
              'warn',
              `Failed to read ref file ${refPath}: ${err instanceof Error ? err.message : String(err)}`
            );
          }
        }
      } catch (err) {
        log(
          'warn',
          `Failed to list refs dir ${refsDir}: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }

    result['references'] = references;
  }

  return JSON.stringify(result);
}

/**
 * skill_search tool — searches the skill index by query, harness, and tag.
 * @deprecated Prefer list_skills + view_skill for progressive disclosure.
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
