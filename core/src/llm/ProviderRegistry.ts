/**
 * ProviderRegistry — maps SERA model names to pi-mono provider configs.
 *
 * Providers are loaded from a JSON config file (default: /app/config/providers.json)
 * on startup. Changes via POST/DELETE /api/providers are written back to the file
 * so they survive restarts.
 *
 * Bootstrap: if LLM_BASE_URL + LLM_MODEL env vars are set and no matching entry
 * exists in the config file, a default openai-compatible provider is registered
 * automatically — useful in development without a config file.
 *
 * Auto-detection: model names that look like known cloud models (gpt-*, claude-*,
 * gemini-*, etc.) are auto-detected without needing an explicit registration.
 * Cloud provider credentials come from standard env vars (OPENAI_API_KEY,
 * ANTHROPIC_API_KEY, …) via pi-mono's getEnvApiKey().
 *
 * @see core/config/providers.json  — example / runtime config
 * @see docs/epics/04-llm-proxy-and-governance.md
 */

import fs from 'fs';
import { Logger } from '../lib/logger.js';

const logger = new Logger('ProviderRegistry');

// ── Types ─────────────────────────────────────────────────────────────────────

/** pi-mono API identifiers that SERA supports routing to. */
export type ProviderApi = 'openai-completions' | 'anthropic-messages';

/**
 * Provider config entry.  Persisted to providers.json.
 *
 * For cloud providers (openai, anthropic, …) you typically only need
 * modelName + api + provider — the API key is read automatically from the
 * corresponding env var (OPENAI_API_KEY, ANTHROPIC_API_KEY, …).
 *
 * For local/custom endpoints (LM Studio, Ollama, vLLM, …) set baseUrl and
 * either apiKey (literal) or apiKeyEnvVar (env var name to read at runtime).
 */
export interface ProviderConfig {
  /** The model name agents use in their API requests. */
  modelName: string;
  /** pi-mono API to route through. Default: 'openai-completions'. */
  api: ProviderApi;
  /** pi-mono provider name — used for env-var key resolution (e.g. 'openai', 'anthropic'). */
  provider?: string;
  /** Override endpoint URL — required for local/custom deployments. */
  baseUrl?: string;
  /**
   * Literal API key.  Prefer apiKeyEnvVar for secrets management.
   * For providers that require no auth (e.g. local LM Studio) use any non-empty
   * placeholder (e.g. 'none').
   */
  apiKey?: string;
  /** Name of an env var to read the API key from at runtime. */
  apiKeyEnvVar?: string;
  /** Human-readable description shown in GET /api/providers. */
  description?: string;
}

interface ConfigFile {
  providers: ProviderConfig[];
}

// ── Registry ──────────────────────────────────────────────────────────────────

const DEFAULT_CONFIG_PATH = process.env.PROVIDERS_CONFIG_PATH ?? '/app/config/providers.json';

export class ProviderRegistry {
  private readonly configs = new Map<string, ProviderConfig>();
  private readonly configPath: string;

  constructor(configPath: string = DEFAULT_CONFIG_PATH) {
    this.configPath = configPath;
    this.loadSync();
    this.bootstrapFromEnv();
  }

  // ── Internal helpers ───────────────────────────────────────────────────────

  private loadSync(): void {
    if (!fs.existsSync(this.configPath)) {
      logger.info(`No providers config at ${this.configPath} — relying on env-var bootstrap and auto-detection`);
      return;
    }
    try {
      const raw = fs.readFileSync(this.configPath, 'utf-8');
      const data = JSON.parse(raw) as ConfigFile;
      for (const cfg of data.providers ?? []) {
        this.configs.set(cfg.modelName, cfg);
      }
      logger.info(`Loaded ${this.configs.size} provider(s) from ${this.configPath}`);
    } catch (err: any) {
      logger.error(`Failed to load providers config at ${this.configPath}: ${err.message}`);
    }
  }

  /**
   * If LLM_BASE_URL + LLM_MODEL are set and no matching entry exists, register
   * a default openai-compatible provider so single-env-var setups work without
   * a config file.
   */
  private bootstrapFromEnv(): void {
    const baseUrl = process.env.LLM_BASE_URL;
    const modelName = process.env.LLM_MODEL ?? 'default';

    if (baseUrl && !this.configs.has(modelName)) {
      const cfg: ProviderConfig = {
        modelName,
        api: 'openai-completions',
        provider: 'default',
        baseUrl,
        ...(process.env.LLM_API_KEY ? { apiKey: process.env.LLM_API_KEY } : {}),
      };
      this.configs.set(modelName, cfg);
      logger.info(`Bootstrapped default provider '${modelName}' → ${baseUrl}`);
    }
  }

  /**
   * Try to infer a provider config from well-known model name prefixes.
   * Returns null when the model name is not recognisable.
   */
  private autoDetect(modelName: string): ProviderConfig | null {
    const lower = modelName.toLowerCase();

    if (lower.startsWith('gpt-') || lower.startsWith('o1') || lower.startsWith('o3') ||
        lower.startsWith('o4') || lower.startsWith('chatgpt-')) {
      return { modelName, api: 'openai-completions', provider: 'openai' };
    }
    if (lower.startsWith('claude-')) {
      return { modelName, api: 'anthropic-messages', provider: 'anthropic' };
    }
    if (lower.startsWith('gemini-')) {
      return { modelName, api: 'openai-completions', provider: 'google' };
    }
    if (lower.startsWith('groq/') || lower.startsWith('groq-')) {
      return { modelName, api: 'openai-completions', provider: 'groq' };
    }
    if (lower.startsWith('mistral-') || lower.startsWith('open-mixtral')) {
      return { modelName, api: 'openai-completions', provider: 'mistral' };
    }

    return null;
  }

  // ── Public API ─────────────────────────────────────────────────────────────

  /**
   * Resolve a model name to its provider config.
   * Tries explicit registry first, then auto-detects from the model name.
   * Throws if no provider can be found.
   */
  resolve(modelName: string): ProviderConfig {
    const explicit = this.configs.get(modelName);
    if (explicit) return explicit;

    const auto = this.autoDetect(modelName);
    if (auto) return auto;

    throw new Error(
      `No provider registered for model '${modelName}'. ` +
      `Register it in ${this.configPath} or via POST /api/providers.`,
    );
  }

  register(config: ProviderConfig): void {
    this.configs.set(config.modelName, config);
  }

  /** Returns true if a provider was removed. */
  unregister(modelName: string): boolean {
    return this.configs.delete(modelName);
  }

  list(): ProviderConfig[] {
    return [...this.configs.values()];
  }

  async save(): Promise<void> {
    const data: ConfigFile = { providers: [...this.configs.values()] };
    await fs.promises.writeFile(this.configPath, JSON.stringify(data, null, 2) + '\n', 'utf-8');
    logger.debug(`Saved ${this.configs.size} provider(s) to ${this.configPath}`);
  }
}
