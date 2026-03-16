import fs from 'fs/promises';
import { existsSync } from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import type { CircleManifest } from './types.js';
import { KNOWN_CIRCLE_FIELDS } from './types.js';
import { ManifestValidationError } from '../agents/manifest/AgentManifestLoader.js';
import type { AgentManifest } from '../agents/manifest/types.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('CircleRegistry');

// ── Registry ────────────────────────────────────────────────────────────────────

export class CircleRegistry {
  private circles: Map<string, CircleManifest> = new Map();
  private projectContexts: Map<string, string> = new Map();

  /**
   * Load and validate a single CIRCLE.yaml file.
   */
  static async loadCircle(filePath: string): Promise<CircleManifest> {
    if (!existsSync(filePath)) {
      throw new ManifestValidationError(`Circle manifest file not found: ${filePath}`);
    }

    const content = await fs.readFile(filePath, 'utf-8');
    const raw = yaml.load(content);
    return CircleRegistry.validateCircle(raw, filePath);
  }

  /**
   * Scan a directory for *.circle.yaml files and load all valid circle manifests.
   */
  static async loadAllCircles(dirPath: string): Promise<CircleManifest[]> {
    if (!existsSync(dirPath)) {
      logger.warn(`Circles directory not found: ${dirPath}`);
      return [];
    }

    const files = (await fs.readdir(dirPath)).filter(f => f.endsWith('.circle.yaml'));
    const circles: CircleManifest[] = [];

    for (const file of files) {
      try {
        const circle = await CircleRegistry.loadCircle(path.join(dirPath, file));
        circles.push(circle);
        logger.info(`Loaded: ${circle.metadata.name} (${file})`);
      } catch (err) {
        logger.error(`Failed to load ${file}:`, (err as Error).message);
      }
    }

    return circles;
  }

  /**
   * Validate a raw parsed YAML object into a typed CircleManifest.
   * Throws ManifestValidationError on invalid input.
   */
  static validateCircle(raw: unknown, source?: string): CircleManifest {
    const ctx = source ? ` (in ${source})` : '';

    if (!raw || typeof raw !== 'object') {
      throw new ManifestValidationError(`Circle manifest must be a YAML object${ctx}`);
    }

    const obj = raw as Record<string, unknown>;

    // ── Reject unknown top-level fields ─────────────────────────────────────
    for (const key of Object.keys(obj)) {
      if (!KNOWN_CIRCLE_FIELDS.has(key)) {
        throw new ManifestValidationError(
          `Unknown top-level field: "${key}"${ctx}`,
          key,
        );
      }
    }

    // ── apiVersion & kind ───────────────────────────────────────────────────
    CircleRegistry.requireString(obj, 'apiVersion', ctx);

    if (obj['kind'] !== 'Circle') {
      throw new ManifestValidationError(
        `"kind" must be "Circle", got "${String(obj['kind'])}"${ctx}`,
        'kind',
      );
    }

    // ── metadata ────────────────────────────────────────────────────────────
    CircleRegistry.requireObject(obj, 'metadata', ctx);
    const meta = obj['metadata'] as Record<string, unknown>;
    CircleRegistry.requireString(meta, 'name', `${ctx} metadata`);
    CircleRegistry.requireString(meta, 'displayName', `${ctx} metadata`);

    // ── agents (required string array) ──────────────────────────────────────
    if (!Array.isArray(obj['agents'])) {
      throw new ManifestValidationError(
        `"agents" must be an array${ctx}`,
        'agents',
      );
    }

    for (const agent of obj['agents'] as unknown[]) {
      if (typeof agent !== 'string') {
        throw new ManifestValidationError(
          `Each entry in "agents" must be a string, got ${typeof agent}${ctx}`,
          'agents',
        );
      }
    }

    // ── knowledge (optional) ────────────────────────────────────────────────
    if (obj['knowledge'] !== undefined) {
      CircleRegistry.requireObject(obj, 'knowledge', ctx);
      const knowledge = obj['knowledge'] as Record<string, unknown>;
      CircleRegistry.requireString(knowledge, 'qdrantCollection', `${ctx} knowledge`);
    }

    // ── channels (optional array) ───────────────────────────────────────────
    if (obj['channels'] !== undefined && !Array.isArray(obj['channels'])) {
      throw new ManifestValidationError(
        `"channels" must be an array${ctx}`,
        'channels',
      );
    }

    return obj as unknown as CircleManifest;
  }

  /**
   * Validate that all agents referenced in a circle have corresponding manifests.
   * Returns a list of missing agent names (empty if all are valid).
   */
  static validateAgentReferences(
    circle: CircleManifest,
    agentManifests: AgentManifest[],
  ): string[] {
    const knownAgents = new Set(agentManifests.map(m => m.metadata.name));
    return circle.agents.filter(name => !knownAgents.has(name));
  }

  // ── Instance Methods ──────────────────────────────────────────────────────────

  /**
   * Register circles from a directory, validating agent references against loaded manifests.
   */
  async loadFromDirectory(circlesDir: string, agentManifests: AgentManifest[] = []): Promise<void> {
    const circles = await CircleRegistry.loadAllCircles(circlesDir);

    for (const circle of circles) {
      // Validate agent references (warn but don't fail)
      const missing = CircleRegistry.validateAgentReferences(circle, agentManifests);
      if (missing.length > 0) {
        logger.warn(
          `Circle "${circle.metadata.name}" references unknown agents: ${missing.join(', ')}`,
        );
      }

      this.circles.set(circle.metadata.name, circle);

      // Load project context if configured
      if (circle.projectContext?.path) {
        await this.loadProjectContext(circle, circlesDir);
      }
    }

    logger.info(`Registered ${circles.length} circles`);
  }

  /**
   * Load the project-context.md for a circle.
   * Resolves the path relative to the circles directory.
   */
  async loadProjectContext(circle: CircleManifest, circlesDir: string): Promise<string | undefined> {
    if (!circle.projectContext?.path) return undefined;

    // Resolve relative to the circles directory
    const contextPath = path.resolve(circlesDir, circle.projectContext.path);

    if (!existsSync(contextPath)) {
      logger.warn(
        `Project context not found for "${circle.metadata.name}": ${contextPath}`,
      );
      return undefined;
    }

    const content = await fs.readFile(contextPath, 'utf-8');
    this.projectContexts.set(circle.metadata.name, content);
    logger.info(`Loaded project context for "${circle.metadata.name}"`);
    return content;
  }

  /**
   * Get the project context content for a circle (or undefined if not loaded).
   */
  getProjectContext(circleName: string): string | undefined {
    return this.projectContexts.get(circleName);
  }

  getCircle(name: string): CircleManifest | undefined {
    return this.circles.get(name);
  }

  listCircles(): CircleManifest[] {
    return Array.from(this.circles.values());
  }

  /**
   * Returns a summary view of all circles (for API responses).
   */
  listCircleSummaries(): Array<{
    name: string;
    displayName: string;
    description?: string;
    agents: string[];
    hasProjectContext: boolean;
    channelCount: number;
  }> {
    return this.listCircles().map(c => {
      const summary: {
        name: string;
        displayName: string;
        description?: string;
        agents: string[];
        hasProjectContext: boolean;
        channelCount: number;
      } = {
        name: c.metadata.name,
        displayName: c.metadata.displayName,
        agents: c.agents,
        hasProjectContext: this.projectContexts.has(c.metadata.name),
        channelCount: c.channels?.length ?? 0,
      };
      if (c.metadata.description !== undefined) {
        summary.description = c.metadata.description;
      }
      return summary;
    });
  }

  // ── Helpers ───────────────────────────────────────────────────────────────────

  private static requireString(
    obj: Record<string, unknown>,
    field: string,
    ctx: string,
  ): void {
    if (obj[field] === undefined || obj[field] === null) {
      throw new ManifestValidationError(
        `Missing required field "${field}"${ctx}`,
        field,
      );
    }
    if (typeof obj[field] !== 'string') {
      throw new ManifestValidationError(
        `Field "${field}" must be a string${ctx}`,
        field,
      );
    }
  }

  private static requireObject(
    obj: Record<string, unknown>,
    field: string,
    ctx: string,
  ): void {
    if (obj[field] === undefined || obj[field] === null) {
      throw new ManifestValidationError(
        `Missing required field "${field}"${ctx}`,
        field,
      );
    }
    if (typeof obj[field] !== 'object' || Array.isArray(obj[field])) {
      throw new ManifestValidationError(
        `Field "${field}" must be an object${ctx}`,
        field,
      );
    }
  }
}
