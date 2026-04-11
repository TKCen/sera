import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { Logger } from '../lib/logger.js';

const logger = new Logger('EmbeddingConfig');

// ── Types ────────────────────────────────────────────────────────────────────

export type EmbeddingProvider = 'ollama' | 'openai' | 'lm-studio' | 'openai-compatible';

export interface EmbeddingConfig {
  provider: EmbeddingProvider;
  model: string;
  baseUrl: string;
  apiKey?: string;
  apiKeyEnvVar?: string;
  dimension: number;
}

// ── Known models ─────────────────────────────────────────────────────────────

export interface KnownEmbeddingModel {
  provider: EmbeddingProvider;
  dimension: number;
  description: string;
}

export const KNOWN_EMBEDDING_MODELS: Record<string, KnownEmbeddingModel> = {
  // Ollama
  'nomic-embed-text': {
    provider: 'ollama',
    dimension: 768,
    description: 'Nomic Embed Text (768d)',
  },
  'mxbai-embed-large': {
    provider: 'ollama',
    dimension: 1024,
    description: 'mxbai Embed Large (1024d)',
  },
  'all-minilm': { provider: 'ollama', dimension: 384, description: 'MiniLM (384d, fast)' },
  'snowflake-arctic-embed': {
    provider: 'ollama',
    dimension: 1024,
    description: 'Snowflake Arctic Embed (1024d)',
  },
  // OpenAI
  'text-embedding-3-small': {
    provider: 'openai',
    dimension: 1536,
    description: 'OpenAI Embed 3 Small (1536d)',
  },
  'text-embedding-3-large': {
    provider: 'openai',
    dimension: 3072,
    description: 'OpenAI Embed 3 Large (3072d)',
  },
  'text-embedding-ada-002': {
    provider: 'openai',
    dimension: 1536,
    description: 'OpenAI Ada 002 (1536d, legacy)',
  },
};

// ── Config path ──────────────────────────────────────────────────────────────

const CONFIG_PATH = process.env.EMBEDDING_CONFIG_PATH ?? '/app/config/embedding.json';

// ── Load / Save ──────────────────────────────────────────────────────────────

/** Load embedding config from file, falling back to env vars for backward compat. */
export function loadEmbeddingConfig(): EmbeddingConfig {
  // Try config file first
  if (existsSync(CONFIG_PATH)) {
    try {
      const raw = JSON.parse(readFileSync(CONFIG_PATH, 'utf-8')) as EmbeddingConfig;
      logger.info(
        `Loaded embedding config from ${CONFIG_PATH}: ${raw.provider}/${raw.model} (${raw.dimension}d)`
      );
      return raw;
    } catch (err) {
      logger.warn(`Failed to parse ${CONFIG_PATH}, falling back to env vars`, err);
    }
  }

  // Fall back to env vars (backward compat with OLLAMA_URL + EMBEDDING_MODEL)
  const ollamaUrl = process.env.OLLAMA_URL;
  const model = process.env.EMBEDDING_MODEL ?? 'nomic-embed-text';
  const known = KNOWN_EMBEDDING_MODELS[model];

  if (ollamaUrl) {
    logger.info(
      `No embedding config file — using env vars: OLLAMA_URL=${ollamaUrl}, model=${model}`
    );
    return {
      provider: 'ollama',
      model,
      baseUrl: ollamaUrl,
      dimension: known?.dimension ?? 768,
    };
  }

  // No config at all — return unconfigured default
  logger.info('No embedding config found — embeddings disabled');
  return {
    provider: 'ollama',
    model,
    baseUrl: '',
    dimension: known?.dimension ?? 768,
  };
}

/** Persist embedding config to file. */
export function saveEmbeddingConfig(config: EmbeddingConfig): void {
  // Strip API key from disk — resolve via apiKeyEnvVar at runtime
  const toSave = { ...config };
  writeFileSync(CONFIG_PATH, JSON.stringify(toSave, null, 2), 'utf-8');
  logger.info(
    `Saved embedding config to ${CONFIG_PATH}: ${config.provider}/${config.model} (${config.dimension}d)`
  );
}

/** Return config with API key masked for API responses. */
export function maskConfig(config: EmbeddingConfig): EmbeddingConfig {
  const masked = { ...config };
  if (masked.apiKey) {
    masked.apiKey = masked.apiKey.slice(0, 4) + '…' + masked.apiKey.slice(-4);
  }
  return masked;
}

/** Resolve the actual API key from config (literal or env var). */
export function resolveApiKey(config: EmbeddingConfig): string | undefined {
  if (config.apiKey) return config.apiKey;
  if (config.apiKeyEnvVar) return process.env[config.apiKeyEnvVar];
  // Standard env var fallback for OpenAI
  if (config.provider === 'openai') return process.env.OPENAI_API_KEY;
  return undefined;
}
