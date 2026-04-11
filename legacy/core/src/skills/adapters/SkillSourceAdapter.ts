import type { SkillFrontMatter } from '../schema.js';

/**
 * An entry in an external skill registry catalog.
 * Returned by search() — lightweight metadata without full content.
 */
export interface ExternalSkillEntry {
  id: string;
  name: string;
  description: string;
  author?: string;
  version?: string;
  tags?: string[];
  downloadUrl?: string;
  source: string;
}

/**
 * A fully resolved skill ready for import.
 * Returned by fetch() — includes frontmatter and markdown content.
 */
export interface ResolvedExternalSkill {
  frontmatter: SkillFrontMatter;
  content: string;
}

/**
 * Adapter interface for external skill registries.
 *
 * Each adapter knows how to search a specific registry and fetch
 * individual skills, mapping them to SERA's SkillDocument format.
 *
 * Adapters are registered with SkillRegistryService at startup.
 */
export interface SkillSourceAdapter {
  /** Unique name for this adapter (e.g. 'clawhub', 'github', 'url') */
  readonly name: string;

  /** Search the registry for skills matching a query string. */
  search(query: string): Promise<ExternalSkillEntry[]>;

  /** Fetch a specific skill by ID and resolve to SERA format. */
  fetch(skillId: string): Promise<ResolvedExternalSkill>;
}
