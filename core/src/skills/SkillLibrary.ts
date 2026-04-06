import fs from 'node:fs/promises';
import path from 'node:path';
import matter from 'gray-matter';
import type { Pool } from 'pg';
import yaml from 'js-yaml';
import chokidar from 'chokidar';
import { SkillFrontMatterSchema, type SkillDocument, type SkillFrontMatter } from './schema.js';
import { SkillPackageSchema, type SkillPackage } from './packageSchema.js';
import { Logger } from '../lib/logger.js';
import type { IntercomService } from '../intercom/IntercomService.js';

const logger = new Logger('SkillLibrary');

export class SkillLibrary {
  private static instance: SkillLibrary;
  private watcher: chokidar.FSWatcher | null = null;
  private intercom: IntercomService | null = null;
  private reloadTimers: Map<string, ReturnType<typeof setTimeout>> = new Map();

  private constructor(private pool: Pool) {}

  public static getInstance(pool: Pool): SkillLibrary {
    if (!SkillLibrary.instance) {
      SkillLibrary.instance = new SkillLibrary(pool);
    }
    return SkillLibrary.instance;
  }

  /** @internal For testing only */
  public static resetInstance(): void {
    SkillLibrary.instance = undefined as unknown as SkillLibrary;
  }

  public setIntercom(intercom: IntercomService): void {
    this.intercom = intercom;
  }

  /**
   * Scans bundled and external skill directories and imports them into the DB.
   */
  async loadSkills(): Promise<{
    added: number;
    updated: number;
    skipped: number;
    errors: string[];
  }> {
    const stats = { added: 0, updated: 0, skipped: 0, errors: [] as string[] };

    const searchPaths = this.getSearchPaths();
    const uniquePaths = Array.from(new Map(searchPaths.map((s) => [s.path, s])).values());

    // 2. Scan and parse
    for (const { path: dirPath, source } of uniquePaths) {
      try {
        // 2.1 Individual skills
        const files = await this.recursiveScan(dirPath, '.md');
        for (const file of files) {
          try {
            const doc = await this.parseSkillFile(file, source);
            if (doc) {
              await this.upsertSkill(doc);
              stats.updated++;
            } else {
              stats.skipped++;
            }
          } catch (err) {
            stats.errors.push(
              `Error parsing skill ${file}: ${err instanceof Error ? err.message : String(err)}`
            );
          }
        }

        // 2.2 Skill packages
        const packageDir = path.join(dirPath, 'packages');
        const pkgFiles = await this.recursiveScan(packageDir, '.yaml');
        for (const file of pkgFiles) {
          try {
            const pkg = await this.parsePackageFile(file);
            if (pkg) {
              await this.upsertPackage(pkg);
              stats.updated++;
            }
          } catch (err) {
            stats.errors.push(
              `Error parsing package ${file}: ${err instanceof Error ? err.message : String(err)}`
            );
          }
        }
      } catch (err: unknown) {
        if ((err as { code?: string }).code !== 'ENOENT') {
          stats.errors.push(
            `Error scanning directory ${dirPath}: ${err instanceof Error ? err.message : String(err)}`
          );
        }
      }
    }

    return stats;
  }

  private getSearchPaths(): { path: string; source: 'bundled' | 'external' }[] {
    const workspaceRoot = process.env.WORKSPACE_DIR || path.resolve(process.cwd(), '..', '..');
    const searchPaths: { path: string; source: 'bundled' | 'external' }[] = [
      { path: path.resolve(process.cwd(), 'skills'), source: 'bundled' },
      { path: path.resolve(workspaceRoot, 'skills'), source: 'bundled' },
    ];

    if (process.env.SKILL_PACK_DIRS) {
      const externalPaths = process.env.SKILL_PACK_DIRS.split(path.delimiter);
      for (const p of externalPaths) {
        if (p.trim()) {
          searchPaths.push({ path: path.resolve(p.trim()), source: 'external' });
        }
      }
    }
    return searchPaths;
  }

  /**
   * Starts watching for file changes in the skill directories.
   * Story 6.5
   */
  public watchSkills(): void {
    if (this.watcher) this.watcher.close();

    const searchPaths = this.getSearchPaths();
    const uniqueDirs = Array.from(new Set(searchPaths.map((s) => s.path)));

    this.watcher = chokidar.watch(uniqueDirs, {
      ignored: /(^|[/\\])\../, // ignore dotfiles
      persistent: true,
      ignoreInitial: true, // we already loaded them
      usePolling: true,
      interval: 100,
    });

    this.watcher.on('all', (event, filePath) => {
      if (event !== 'add' && event !== 'change') return;

      // Debounce: wait 300ms after last change before processing.
      // This handles partial writes where the OS fires multiple events.
      const existing = this.reloadTimers.get(filePath);
      if (existing) clearTimeout(existing);

      this.reloadTimers.set(
        filePath,
        setTimeout(async () => {
          this.reloadTimers.delete(filePath);

          const ext = path.extname(filePath);
          if (ext === '.md') {
            // Individual skill reload
            try {
              const source = filePath.includes('skills' + path.sep + 'builtin')
                ? 'bundled'
                : 'external';
              const doc = await this.parseSkillFile(filePath, source as 'bundled' | 'external');
              if (doc) {
                await this.upsertSkill(doc);
                logger.info(`Skill hot-reloaded: ${doc.name} v${doc.version}`);
                this.emitReloadEvent('skill', doc.name, doc.version);
              }
            } catch (err: unknown) {
              logger.error(`Failed to hot-reload skill ${filePath}:`, err);
            }
          } else if (ext === '.yaml' && filePath.includes('packages')) {
            // Package reload
            try {
              const pkg = await this.parsePackageFile(filePath);
              if (pkg) {
                await this.upsertPackage(pkg);
                logger.info(`Skill package hot-reloaded: ${pkg.name} v${pkg.version}`);
                this.emitReloadEvent('package', pkg.name, pkg.version);
              }
            } catch (err: unknown) {
              logger.error(`Failed to hot-reload package ${filePath}:`, err);
            }
          }
        }, 300)
      );
    });

    logger.info(`Watching skills directory for changes...`);
  }

  private emitReloadEvent(type: 'skill' | 'package', name: string, version: string): void {
    if (this.intercom) {
      this.intercom
        .publish('system.skill-reloaded', {
          type,
          name,
          version,
          timestamp: new Date().toISOString(),
        })
        .catch((err) => logger.warn('Failed to publish skill-reloaded event:', err));
    }
  }

  public stopWatching(): void {
    for (const timer of this.reloadTimers.values()) {
      clearTimeout(timer);
    }
    this.reloadTimers.clear();
    if (this.watcher) {
      this.watcher.close();
      this.watcher = null;
    }
  }

  private async recursiveScan(dir: string, ext: string): Promise<string[]> {
    try {
      const entries = await fs.readdir(dir, { withFileTypes: true });
      const files = await Promise.all(
        entries.map(async (entry) => {
          const res = path.resolve(dir, entry.name);
          return entry.isDirectory() ? this.recursiveScan(res, ext) : res;
        })
      );
      return Array.prototype.concat(...files).filter((f: string) => f.endsWith(ext));
    } catch {
      return [];
    }
  }

  private async parseSkillFile(
    filePath: string,
    source: 'bundled' | 'external'
  ): Promise<SkillDocument | null> {
    const fileContent = await fs.readFile(filePath, 'utf-8');
    const { data, content } = matter(fileContent);

    const validation = SkillFrontMatterSchema.safeParse(data);
    if (!validation.success) {
      logger.warn(`Invalid front-matter in ${filePath}:`, validation.error.format());
      return null;
    }

    return {
      ...validation.data,
      content: content.trim(),
      source,
    };
  }

  private async parsePackageFile(filePath: string): Promise<SkillPackage | null> {
    const fileContent = await fs.readFile(filePath, 'utf-8');
    const data = yaml.load(fileContent);

    const validation = SkillPackageSchema.safeParse(data);
    if (!validation.success) {
      logger.warn(`Invalid package in ${filePath}:`, validation.error.format());
      return null;
    }

    return validation.data;
  }

  private async upsertSkill(doc: SkillDocument): Promise<void> {
    await this.pool.query(
      `INSERT INTO skills (
        skill_id, name, version, description, triggers, requires, conflicts, max_tokens, content, source, category, tags, applies_to, updated_at
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NOW())
      ON CONFLICT (name, version) DO UPDATE SET
        skill_id = EXCLUDED.skill_id,
        description = EXCLUDED.description,
        triggers = EXCLUDED.triggers,
        requires = EXCLUDED.requires,
        conflicts = EXCLUDED.conflicts,
        max_tokens = EXCLUDED.max_tokens,
        content = EXCLUDED.content,
        source = EXCLUDED.source,
        category = EXCLUDED.category,
        tags = EXCLUDED.tags,
        applies_to = EXCLUDED.applies_to,
        updated_at = NOW()`,
      [
        doc.id || null,
        doc.name,
        doc.version,
        doc.description,
        JSON.stringify(doc.triggers),
        JSON.stringify(doc.requires || []),
        JSON.stringify(doc.conflicts || []),
        doc.maxTokens || null,
        doc.content,
        doc.source,
        doc.category || null,
        JSON.stringify(doc.tags || []),
        JSON.stringify(doc['applies-to'] || []),
      ]
    );
  }

  private async upsertPackage(pkg: SkillPackage): Promise<void> {
    await this.pool.query(
      `INSERT INTO skill_packages (name, version, description, skills, updated_at)
      VALUES ($1, $2, $3, $4, NOW())
      ON CONFLICT (name, version) DO UPDATE SET
        description = EXCLUDED.description,
        skills = EXCLUDED.skills,
        updated_at = NOW()`,
      [pkg.name, pkg.version, pkg.description || null, JSON.stringify(pkg.skills)]
    );
  }

  async getSkill(name: string, version?: string): Promise<SkillDocument | null> {
    let query = 'SELECT * FROM skills WHERE name = $1';
    const params = [name];
    if (version) {
      query += ' AND version = $2';
      params.push(version);
    } else {
      query += ' ORDER BY version DESC LIMIT 1';
    }

    const { rows } = await this.pool.query(query, params);
    if (rows.length === 0) return null;

    const row = rows[0];
    return {
      id: row.skill_id,
      name: row.name,
      version: row.version,
      description: row.description,
      triggers: row.triggers,
      requires: row.requires,
      conflicts: row.conflicts,
      maxTokens: row.max_tokens,
      content: row.content,
      source: row.source,
      category: row.category,
      tags: row.tags,
      'applies-to': row.applies_to,
    };
  }

  async getPackage(name: string, version?: string): Promise<SkillPackage | null> {
    let query = 'SELECT * FROM skill_packages WHERE name = $1';
    const params = [name];
    if (version) {
      query += ' AND version = $2';
      params.push(version);
    } else {
      query += ' ORDER BY version DESC LIMIT 1';
    }

    const { rows } = await this.pool.query(query, params);
    if (rows.length === 0) return null;

    const row = rows[0];
    return {
      name: row.name,
      version: row.version,
      description: row.description,
      skills: row.skills,
    };
  }

  /**
   * Fetches the latest version of multiple skill packages in a single query.
   */
  async getPackages(names: string[]): Promise<SkillPackage[]> {
    if (names.length === 0) return [];

    const { rows } = await this.pool.query(
      'SELECT DISTINCT ON (name) name, version, description, skills FROM skill_packages WHERE name = ANY($1) ORDER BY name, version DESC',
      [names]
    );

    return rows.map((row) => ({
      name: row.name,
      version: row.version,
      description: row.description,
      skills: row.skills,
    }));
  }

  async listSkills(): Promise<Array<SkillFrontMatter & { maxTokens?: number; source?: string }>> {
    const { rows } = await this.pool.query(
      'SELECT name, version, description, triggers, category, tags, max_tokens, source FROM skills ORDER BY name, version DESC'
    );
    return rows.map((row) => ({
      name: row.name,
      version: row.version,
      description: row.description,
      triggers: row.triggers,
      category: row.category,
      tags: row.tags,
      ...(row.max_tokens != null ? { maxTokens: row.max_tokens as number } : {}),
      ...(row.source ? { source: row.source as string } : {}),
    }));
  }

  async listPackages(): Promise<SkillPackage[]> {
    const { rows } = await this.pool.query(
      'SELECT name, version, description, skills FROM skill_packages ORDER BY name, version DESC'
    );
    return rows;
  }

  // ── Public CRUD (for API-driven skill creation) ────────────────────────────

  /**
   * Create or update a guidance skill from API input.
   * Validates frontmatter against the schema before persisting.
   */
  async createSkill(
    frontmatter: SkillFrontMatter,
    content: string,
    source?: string
  ): Promise<void> {
    const validation = SkillFrontMatterSchema.safeParse(frontmatter);
    if (!validation.success) {
      throw new Error(`Invalid skill frontmatter: ${validation.error.message}`);
    }
    await this.upsertSkill({ ...validation.data, content, source: source ?? 'external' });
  }

  /**
   * Delete a guidance skill by name (and optionally version).
   * Returns true if a row was deleted.
   */
  async deleteSkill(name: string, version?: string): Promise<boolean> {
    let query = 'DELETE FROM skills WHERE name = $1';
    const params: string[] = [name];
    if (version) {
      query += ' AND version = $2';
      params.push(version);
    }
    const result = await this.pool.query(query, params);
    return (result.rowCount ?? 0) > 0;
  }
}
