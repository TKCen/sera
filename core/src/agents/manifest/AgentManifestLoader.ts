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
    public readonly field?: string
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
        throw new ManifestValidationError(`Unknown top-level field: "${key}"${ctx}`, key);
      }
    }

    // ── apiVersion & kind ─────────────────────────────────────────────────────
    AgentManifestLoader.requireString(obj, 'apiVersion', ctx);

    if (obj['kind'] !== 'Agent') {
      throw new ManifestValidationError(
        `"kind" must be "Agent", got "${String(obj['kind'])}"${ctx}`,
        'kind'
      );
    }

    // ── metadata ──────────────────────────────────────────────────────────────
    AgentManifestLoader.requireObject(obj, 'metadata', ctx);
    const meta = obj['metadata'] as Record<string, unknown>;
    AgentManifestLoader.requireString(meta, 'name', `${ctx} metadata`);

    if (meta['displayName'] !== undefined) {
      AgentManifestLoader.requireString(meta, 'displayName', `${ctx} metadata`);
    }
    if (meta['circle'] !== undefined) {
      AgentManifestLoader.requireString(meta, 'circle', `${ctx} metadata`);
    }

    if (meta['additionalCircles'] !== undefined) {
      if (
        !Array.isArray(meta['additionalCircles']) ||
        !meta['additionalCircles'].every((c) => typeof c === 'string')
      ) {
        throw new ManifestValidationError(
          `"additionalCircles" must be an array of strings${ctx}`,
          'metadata.additionalCircles'
        );
      }
    }

    // Default icon
    if (meta['icon'] === undefined) {
      meta['icon'] = '🤖';
    }

    // Tier validation — optional when using spec-wrapped format
    const tier = meta['tier'];
    if (tier !== undefined && !VALID_TIERS.includes(tier as SecurityTier)) {
      throw new ManifestValidationError(
        `Invalid security tier: ${String(tier)}. Must be one of: ${VALID_TIERS.join(', ')}${ctx}`,
        'metadata.tier'
      );
    }

    // Detect format: spec-wrapped (new) vs flat (legacy)
    const isSpecWrapped = obj['spec'] !== undefined && typeof obj['spec'] === 'object';

    if (!isSpecWrapped) {
      // ── identity (flat format only) ────────────────────────────────────────
      AgentManifestLoader.requireObject(obj, 'identity', ctx);
      const identity = obj['identity'] as Record<string, unknown>;
      AgentManifestLoader.requireString(identity, 'role', `${ctx} identity`);
      AgentManifestLoader.requireString(identity, 'description', `${ctx} identity`);

      // ── model (flat format only) ───────────────────────────────────────────
      AgentManifestLoader.requireObject(obj, 'model', ctx);
      const model = obj['model'] as Record<string, unknown>;
      AgentManifestLoader.requireString(model, 'provider', `${ctx} model`);
      AgentManifestLoader.requireString(model, 'name', `${ctx} model`);
    }

    // ── resources ─────────────────────────────────────────────────────────────
    if (obj['resources']) {
      const res = obj['resources'] as Record<string, unknown>;
      if (res['maxLlmTokensPerHour'] !== undefined) {
        if (typeof res['maxLlmTokensPerHour'] !== 'number' || res['maxLlmTokensPerHour'] <= 0) {
          throw new ManifestValidationError(
            `"maxLlmTokensPerHour" must be a positive number${ctx}`,
            'resources.maxLlmTokensPerHour'
          );
        }
      }
    }

    // ── permissions ───────────────────────────────────────────────────────────
    if (obj['permissions']) {
      const perms = obj['permissions'] as Record<string, unknown>;
      if (perms['canExec'] !== undefined && typeof perms['canExec'] !== 'boolean') {
        throw new ManifestValidationError(
          `"canExec" must be a boolean${ctx}`,
          'permissions.canExec'
        );
      }
      if (
        perms['canSpawnSubagents'] !== undefined &&
        typeof perms['canSpawnSubagents'] !== 'boolean'
      ) {
        throw new ManifestValidationError(
          `"canSpawnSubagents" must be a boolean${ctx}`,
          'permissions.canSpawnSubagents'
        );
      }
    }

    // ── logging ───────────────────────────────────────────────────────────────
    if (obj['logging']) {
      const logging = obj['logging'] as Record<string, unknown>;
      if (logging['commands'] !== undefined && typeof logging['commands'] !== 'boolean') {
        throw new ManifestValidationError(`"commands" must be a boolean${ctx}`, 'logging.commands');
      }
    }

    // ── memory search config ──────────────────────────────────────────────────
    const mem = obj['memory'] || (isSpecWrapped && (obj['spec'] as any).memory);
    if (mem && mem.search) {
      const s = mem.search;
      if (typeof s !== 'object') {
        throw new ManifestValidationError(
          `"memory.search" must be an object${ctx}`,
          'memory.search'
        );
      }
      if (s.vectorWeight !== undefined && typeof s.vectorWeight !== 'number') {
        throw new ManifestValidationError(
          `"vectorWeight" must be a number${ctx}`,
          'memory.search.vectorWeight'
        );
      }
      if (s.textWeight !== undefined && typeof s.textWeight !== 'number') {
        throw new ManifestValidationError(
          `"textWeight" must be a number${ctx}`,
          'memory.search.textWeight'
        );
      }
      if (s.mmr && typeof s.mmr !== 'object') {
        throw new ManifestValidationError(`"mmr" must be an object${ctx}`, 'memory.search.mmr');
      }
      if (s.temporalDecay && typeof s.temporalDecay !== 'object') {
        throw new ManifestValidationError(
          `"temporalDecay" must be an object${ctx}`,
          'memory.search.temporalDecay'
        );
      }
    }

    // ── capabilities ──────────────────────────────────────────────────────────
    if (obj['capabilities']) {
      if (
        !Array.isArray(obj['capabilities']) ||
        !obj['capabilities'].every((c) => typeof c === 'string')
      ) {
        throw new ManifestValidationError(
          `"capabilities" must be an array of strings`,
          'capabilities'
        );
      }
    }

    // ── schedules ─────────────────────────────────────────────────────────────
    if (obj['schedules']) {
      if (!Array.isArray(obj['schedules'])) {
        throw new ManifestValidationError(
          `"schedules" must be an array of objects${ctx}`,
          'schedules'
        );
      }
      for (let i = 0; i < obj['schedules'].length; i++) {
        const s = obj['schedules'][i] as Record<string, unknown>;
        const sCtx = `${ctx} schedules[${i}]`;
        AgentManifestLoader.requireString(s, 'name', sCtx);
        AgentManifestLoader.requireString(s, 'type', sCtx);
        if (s['type'] !== 'cron' && s['type'] !== 'once') {
          throw new ManifestValidationError(
            `Schedule type must be "cron" or "once", got "${String(s['type'])}"${sCtx}`,
            `schedules[${i}].type`
          );
        }
        AgentManifestLoader.requireString(s, 'expression', sCtx);
        AgentManifestLoader.requireString(s, 'task', sCtx);
        if (s['status'] != null && s['status'] !== 'active' && s['status'] !== 'paused') {
          throw new ManifestValidationError(
            `Schedule status must be "active" or "paused", got "${String(s['status'])}"${sCtx}`,
            `schedules[${i}].status`
          );
        }
        if (s['category'] != null && typeof s['category'] !== 'string') {
          throw new ManifestValidationError(
            `Schedule category must be a string${sCtx}`,
            `schedules[${i}].category`
          );
        }
      }
    }

    // ── contextFiles & notes (within spec) ────────────────────────────────────
    const spec = obj['spec'] as Record<string, unknown> | undefined;
    if (spec) {
      if (spec['contextFiles']) {
        if (!Array.isArray(spec['contextFiles'])) {
          throw new ManifestValidationError(
            `"contextFiles" must be an array of objects${ctx}`,
            'spec.contextFiles'
          );
        }
        for (let i = 0; i < spec['contextFiles'].length; i++) {
          const f = spec['contextFiles'][i] as Record<string, unknown>;
          const fCtx = `${ctx} spec.contextFiles[${i}]`;
          AgentManifestLoader.requireString(f, 'path', fCtx);
          AgentManifestLoader.requireString(f, 'label', fCtx);
          if (f['maxTokens'] !== undefined && typeof f['maxTokens'] !== 'number') {
            throw new ManifestValidationError(
              `"maxTokens" must be a number${fCtx}`,
              `spec.contextFiles[${i}].maxTokens`
            );
          }
          if (
            f['priority'] !== undefined &&
            !['high', 'normal', 'low'].includes(f['priority'] as string)
          ) {
            throw new ManifestValidationError(
              `"priority" must be one of: high, normal, low${fCtx}`,
              `spec.contextFiles[${i}].priority`
            );
          }
        }
      }

      if (spec['notes'] !== undefined && typeof spec['notes'] !== 'string') {
        throw new ManifestValidationError(`"notes" must be a string${ctx}`, 'spec.notes');
      }
    }

    // ── Construct validated manifest ──────────────────────────────────────────

    // Type assertions are safe here because we've manually validated the required fields
    // using the AgentManifestLoader.require* helpers above.
    // Cast through unknown to avoid returning a brand-new object
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return obj as any as AgentManifest;
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────

  private static requireString(obj: Record<string, unknown>, field: string, ctx: string): void {
    if (obj[field] === undefined || obj[field] === null) {
      throw new ManifestValidationError(`Missing required field "${field}"${ctx}`, field);
    }
    if (typeof obj[field] !== 'string') {
      throw new ManifestValidationError(`Field "${field}" must be a string${ctx}`, field);
    }
  }

  private static requireObject(obj: Record<string, unknown>, field: string, ctx: string): void {
    if (obj[field] === undefined || obj[field] === null) {
      throw new ManifestValidationError(`Missing required field "${field}"${ctx}`, field);
    }
    if (typeof obj[field] !== 'object' || Array.isArray(obj[field])) {
      throw new ManifestValidationError(`Field "${field}" must be an object${ctx}`, field);
    }
  }
}
