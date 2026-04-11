/**
 * ClawHub Adapter — search and import skills from clawhub.ai.
 *
 * ClawHub hosts 3,000+ community skills in SKILL.md format (prompt-only).
 * API reference: https://clawhub.ai/api/v1/
 *
 * Endpoints used:
 *   Search:   GET /api/v1/search?q=...&limit=20
 *   Browse:   GET /api/v1/skills?limit=20&sort=trending
 *   Detail:   GET /api/v1/skills/{slug}
 *   File:     GET /api/v1/skills/{slug}/file?path=SKILL.md
 */

import matter from 'gray-matter';
import { Logger } from '../../lib/logger.js';
import type { SkillFrontMatter } from '../schema.js';
import type {
  SkillSourceAdapter,
  ExternalSkillEntry,
  ResolvedExternalSkill,
} from './SkillSourceAdapter.js';

const logger = new Logger('ClawHubAdapter');

const CLAWHUB_BASE = 'https://clawhub.ai/api/v1';
const USER_AGENT = 'SERA/0.1';

// ── ClawHub API response types ───────────────────────────────────────────────

interface ClawHubSearchEntry {
  score: number;
  slug: string;
  displayName: string;
  summary: string;
  version?: string;
  updatedAt: number;
}

interface ClawHubSearchResponse {
  results: ClawHubSearchEntry[];
}

interface ClawHubBrowseEntry {
  slug: string;
  displayName: string;
  summary: string;
  tags: Record<string, string>;
  stats: {
    downloads: number;
    stars: number;
    installsAllTime: number;
  };
  latestVersion?: { version: string };
}

interface ClawHubBrowseResponse {
  items: ClawHubBrowseEntry[];
}

// ── Adapter ──────────────────────────────────────────────────────────────────

export class ClawHubAdapter implements SkillSourceAdapter {
  readonly name = 'clawhub';

  async search(query: string): Promise<ExternalSkillEntry[]> {
    try {
      const url = `${CLAWHUB_BASE}/search?q=${encodeURIComponent(query)}&limit=20`;
      const resp = await fetch(url, {
        headers: { 'User-Agent': USER_AGENT },
        signal: AbortSignal.timeout(15000),
      });

      if (!resp.ok) {
        logger.warn(`ClawHub search returned ${resp.status}`);
        return [];
      }

      const data = (await resp.json()) as ClawHubSearchResponse;
      return data.results.map((r) => ({
        id: r.slug,
        name: r.displayName || r.slug,
        description: r.summary,
        ...(r.version ? { version: r.version } : {}),
        source: 'clawhub',
      }));
    } catch (err) {
      logger.error('ClawHub search failed:', err);
      return [];
    }
  }

  /**
   * Browse trending skills (used when search query is empty).
   */
  async browse(): Promise<ExternalSkillEntry[]> {
    try {
      const url = `${CLAWHUB_BASE}/skills?limit=20&sort=trending`;
      const resp = await fetch(url, {
        headers: { 'User-Agent': USER_AGENT },
        signal: AbortSignal.timeout(15000),
      });

      if (!resp.ok) {
        logger.warn(`ClawHub browse returned ${resp.status}`);
        return [];
      }

      const data = (await resp.json()) as ClawHubBrowseResponse;
      return data.items.map((e) => {
        const version = e.latestVersion?.version ?? e.tags['latest'];
        return {
          id: e.slug,
          name: e.displayName || e.slug,
          description: e.summary,
          ...(version ? { version } : {}),
          tags: Object.keys(e.tags),
          source: 'clawhub',
        };
      });
    } catch (err) {
      logger.error('ClawHub browse failed:', err);
      return [];
    }
  }

  async fetch(skillId: string): Promise<ResolvedExternalSkill> {
    // Fetch the SKILL.md file from ClawHub
    const url = `${CLAWHUB_BASE}/skills/${encodeURIComponent(skillId)}/file?path=SKILL.md`;
    const resp = await fetch(url, {
      headers: { 'User-Agent': USER_AGENT },
      signal: AbortSignal.timeout(15000),
    });

    if (!resp.ok) {
      throw new Error(`Failed to fetch SKILL.md for "${skillId}" from ClawHub: ${resp.status}`);
    }

    const raw = await resp.text();

    // Parse the SKILL.md frontmatter + content using gray-matter
    const { data, content } = matter(raw);

    // Map ClawHub SKILL.md format → SERA SkillFrontMatter
    const frontmatter: SkillFrontMatter = {
      name: (data.name as string) || skillId,
      version: '1.0.0',
      description: (data.description as string) || '',
      triggers: this.deriveTriggers(data, content),
      ...(data.category ? { category: data.category as string } : {}),
      ...(Array.isArray(data.tags) ? { tags: data.tags as string[] } : {}),
    };

    logger.info(`Fetched ClawHub skill "${skillId}" → "${frontmatter.name}"`);

    return { frontmatter, content: content.trim() };
  }

  /**
   * Derive trigger keywords from ClawHub metadata and content.
   */
  private deriveTriggers(data: Record<string, unknown>, content: string): string[] {
    // Use tags if available
    if (Array.isArray(data.tags) && data.tags.length > 0) {
      return data.tags as string[];
    }

    // Otherwise extract from name and first heading
    const name = (data.name as string) || '';
    const triggers = name
      .toLowerCase()
      .split(/[\s-]+/)
      .filter((w) => w.length > 2);

    // Extract first # heading from content
    const headingMatch = content.match(/^#\s+(.+)$/m);
    if (headingMatch) {
      const words = headingMatch[1]!
        .toLowerCase()
        .split(/[\s-]+/)
        .filter((w) => w.length > 2);
      for (const w of words) {
        if (!triggers.includes(w)) triggers.push(w);
      }
    }

    return triggers.length > 0 ? triggers : ['general'];
  }
}
