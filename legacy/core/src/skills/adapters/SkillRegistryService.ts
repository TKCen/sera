/**
 * SkillRegistryService — orchestrates skill source adapters.
 *
 * Provides a unified search/import interface across multiple external
 * skill registries (ClawHub, URL, etc).
 */

import type { Pool } from 'pg';
import { Logger } from '../../lib/logger.js';
import { SkillLibrary } from '../SkillLibrary.js';
import type { SkillSourceAdapter, ExternalSkillEntry } from './SkillSourceAdapter.js';
import { ClawHubAdapter } from './ClawHubAdapter.js';

const logger = new Logger('SkillRegistryService');

export class SkillRegistryService {
  private adapters = new Map<string, SkillSourceAdapter>();
  private static instance: SkillRegistryService | undefined;

  private constructor(private pool: Pool) {
    // Register default adapters
    this.registerAdapter(new ClawHubAdapter());
  }

  static getInstance(pool: Pool): SkillRegistryService {
    if (!SkillRegistryService.instance) {
      SkillRegistryService.instance = new SkillRegistryService(pool);
    }
    return SkillRegistryService.instance;
  }

  registerAdapter(adapter: SkillSourceAdapter): void {
    this.adapters.set(adapter.name, adapter);
    logger.info(`Registered skill source adapter: ${adapter.name}`);
  }

  /**
   * Search across all adapters (or a specific one).
   */
  async search(query: string, source?: string): Promise<ExternalSkillEntry[]> {
    if (source) {
      const adapter = this.adapters.get(source);
      if (!adapter) {
        throw new Error(`Unknown skill source: ${source}`);
      }
      if (
        !query &&
        'browse' in adapter &&
        typeof (adapter as ClawHubAdapter).browse === 'function'
      ) {
        return (adapter as ClawHubAdapter).browse();
      }
      return adapter.search(query);
    }

    // Search all adapters in parallel
    const results = await Promise.allSettled(
      [...this.adapters.values()].map((a) => a.search(query))
    );

    const combined: ExternalSkillEntry[] = [];
    for (const result of results) {
      if (result.status === 'fulfilled') {
        combined.push(...result.value);
      }
    }
    return combined;
  }

  /**
   * Import a skill from an external registry into the SERA DB.
   */
  async importSkill(source: string, skillId: string): Promise<void> {
    const adapter = this.adapters.get(source);
    if (!adapter) {
      throw new Error(`Unknown skill source: ${source}`);
    }

    const resolved = await adapter.fetch(skillId);
    const library = SkillLibrary.getInstance(this.pool);

    await library.createSkill(resolved.frontmatter, resolved.content, `external:${source}`);

    logger.info(`Imported skill "${resolved.frontmatter.name}" from ${source}`);
  }

  getAdapterNames(): string[] {
    return [...this.adapters.keys()];
  }
}
