import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import type { AgentManifest, SecurityTier } from './types.js';
import { KNOWN_TOP_LEVEL_FIELDS, VALID_TIERS } from './types.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('AgentManifestLoader');

// ── Validation Errors ───────────────────────────────────────────────────────────

export class ManifestValidationError extends Error {
  constructor(
    message: string,
    public readonly field?: string,
  ) {
    super(message);
    this.name = 'ManifestValidationError';
  }
}

// ── Loader ──────────────────────────────────────────────────────────────────────

export class AgentManifestLoader {
  /**
   * Load and validate a single AGENT.yaml file.
   */
  static loadManifest(filePath: string): AgentManifest {
    if (!fs.existsSync(filePath)) {
      throw new ManifestValidationError(`Manifest file not found: ${filePath}`);
    }

    const raw = yaml.load(fs.readFileSync(filePath, 'utf-8'));
    return AgentManifestLoader.validateManifest(raw, filePath);
  }

  /**
   * Scan a directory for *.agent.yaml files and AGENT.yaml files in subdirectories,
   * then load all valid manifests.
   */
  static loadAllManifests(dirPath: string): AgentManifest[] {
    console.time('[AgentManifestLoader] loadAllManifests');
    if (!fs.existsSync(dirPath)) {
      logger.warn(`Agents directory not found: ${dirPath}`);
      console.timeEnd('[AgentManifestLoader] loadAllManifests');
      return [];
    }

    const entries = fs.readdirSync(dirPath, { withFileTypes: true });
    const manifests: AgentManifest[] = [];

    for (const entry of entries) {
      let filePath: string | undefined;

      if (entry.isFile() && entry.name.endsWith('.agent.yaml')) {
        filePath = path.join(dirPath, entry.name);
      } else if (entry.isDirectory()) {
        const subDirAgentFile = path.join(dirPath, entry.name, 'AGENT.yaml');
        if (fs.existsSync(subDirAgentFile)) {
          filePath = subDirAgentFile;
        }
      }

      if (filePath) {
        try {
          const manifest = AgentManifestLoader.loadManifest(filePath);
          manifests.push(manifest);
          logger.info(`Loaded: ${manifest.metadata.name} (${path.relative(dirPath, filePath)})`);
        } catch (err) {
          logger.error(`Failed to load ${filePath}:`, (err as Error).message);
        }
      }
    }

    console.timeEnd('[AgentManifestLoader] loadAllManifests');
    return manifests;
  }

  /**
   * Validate a raw parsed YAML object into a typed AgentManifest.
   * Throws ManifestValidationError on invalid input.
   */
  static validateManifest(raw: unknown, source?: string): AgentManifest {
    const ctx = source ? ` (in ${source})` : '';

    if (!raw || typeof raw !== 'object') {
      throw new ManifestValidationError(`Manifest must be a YAML object${ctx}`);
    }

    const obj = raw as Record<string, unknown>;

    // ── Reject unknown top-level fields ───────────────────────────────────────
    for (const key of Object.keys(obj)) {
      if (!KNOWN_TOP_LEVEL_FIELDS.has(key)) {
        throw new ManifestValidationError(
          `Unknown top-level field: "${key}"${ctx}`,
          key,
        );
      }
    }

    // ── apiVersion & kind ─────────────────────────────────────────────────────
    AgentManifestLoader.requireString(obj, 'apiVersion', ctx);

    if (obj['kind'] !== 'Agent') {
      throw new ManifestValidationError(
        `"kind" must be "Agent", got "${String(obj['kind'])}"${ctx}`,
        'kind',
      );
    }

    // ── metadata ──────────────────────────────────────────────────────────────
    AgentManifestLoader.requireObject(obj, 'metadata', ctx);
    const meta = obj['metadata'] as Record<string, unknown>;
    AgentManifestLoader.requireString(meta, 'name', `${ctx} metadata`);
    AgentManifestLoader.requireString(meta, 'displayName', `${ctx} metadata`);
    AgentManifestLoader.requireString(meta, 'circle', `${ctx} metadata`);

    // Default icon
    if (meta['icon'] === undefined) {
      meta['icon'] = '🤖';
    }

    // Tier validation
    const tier = meta['tier'];
    if (tier === undefined) {
      throw new ManifestValidationError(
        `Missing required field "tier" in metadata${ctx}`,
        'metadata.tier',
      );
    }
    if (!VALID_TIERS.includes(tier as SecurityTier)) {
      throw new ManifestValidationError(
        `Invalid security tier: ${String(tier)}. Must be one of: ${VALID_TIERS.join(', ')}${ctx}`,
        'metadata.tier',
      );
    }

    // ── identity ──────────────────────────────────────────────────────────────
    AgentManifestLoader.requireObject(obj, 'identity', ctx);
    const identity = obj['identity'] as Record<string, unknown>;
    AgentManifestLoader.requireString(identity, 'role', `${ctx} identity`);
    AgentManifestLoader.requireString(identity, 'description', `${ctx} identity`);

    // ── model ─────────────────────────────────────────────────────────────────
    AgentManifestLoader.requireObject(obj, 'model', ctx);
    const model = obj['model'] as Record<string, unknown>;
    AgentManifestLoader.requireString(model, 'provider', `${ctx} model`);
    AgentManifestLoader.requireString(model, 'name', `${ctx} model`);

    // ── Construct validated manifest ──────────────────────────────────────────
    return obj as unknown as AgentManifest;
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────

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
