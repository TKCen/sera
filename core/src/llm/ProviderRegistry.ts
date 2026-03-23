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
  provider?: string | undefined;
  /** Override endpoint URL — required for local/custom deployments. */
  baseUrl?: string | undefined;
  /**
   * Literal API key.  Prefer apiKeyEnvVar for secrets management.
   * For providers that require no auth (e.g. local LM Studio) use any non-empty
   * placeholder (e.g. 'none').
   */
  apiKey?: string | undefined;
  /** Name of an env var to read the API key from at runtime. */
  apiKeyEnvVar?: string | undefined;
  /** Human-readable description shown in GET /api/providers. */
  description?: string | undefined;
  /** ID of the dynamic provider that registered this model (internal). */
  dynamicProviderId?: string | undefined;
  /** Whether this provider is enabled. Defaults to true. */
  enabled?: boolean | undefined;
}

export interface DynamicProviderConfig {
  id: string;
  name: string;
  type: 'lm-studio';
  baseUrl: string;
  apiKey?: string | undefined;
  enabled: boolean;
  intervalMs: number;
  description?: string | undefined;
}

export interface DynamicProviderStatus {
  id: string;
  lastCheck?: string | undefined;
  status: 'ok' | 'error';
  error?: string | undefined;
  discoveredModels: string[];
}

interface ConfigFile {
  providers: ProviderConfig[];
}

// ── Registry ──────────────────────────────────────────────────────────────────

const DEFAULT_CONFIG_PATH = process.env.PROVIDERS_CONFIG_PATH ?? '/app/config/providers.json';

export class ProviderRegistry {
  private readonly configs = new Map<string, ProviderConfig>();
  private readonly configPath: string;
  private defaultModelName: string | null = null;

  constructor(configPath: string = DEFAULT_CONFIG_PATH) {
    this.configPath = configPath;
    this.loadSync();
    this.bootstrapFromEnv();
    this.initDefaultModel();
  }

  // ── Internal helpers ───────────────────────────────────────────────────────

  private loadSync(): void {
    if (!fs.existsSync(this.configPath)) {
      logger.info(
        `No providers config at ${this.configPath} — relying on env-var bootstrap and auto-detection`
      );
      return;
    }
    try {
      const raw = fs.readFileSync(this.configPath, 'utf-8');
      const data = JSON.parse(raw) as ConfigFile;
      for (const cfg of data.providers ?? []) {
        this.configs.set(cfg.modelName, cfg);
      }
      logger.info(`Loaded ${this.configs.size} provider(s) from ${this.configPath}`);
    } catch (err: unknown) {
      logger.error(
        `Failed to load providers config at ${this.configPath}: ${(err as Error).message}`
      );
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

    if (
      lower.startsWith('gpt-') ||
      lower.startsWith('o1') ||
      lower.startsWith('o3') ||
      lower.startsWith('o4') ||
      lower.startsWith('chatgpt-')
    ) {
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

  /**
   * Set the default model from DEFAULT_MODEL env var, or fall back to
   * the first registered provider entry.
   */
  private initDefaultModel(): void {
    const envDefault = process.env.DEFAULT_MODEL;
    if (envDefault && this.configs.has(envDefault)) {
      this.defaultModelName = envDefault;
      logger.info(`Default model set from DEFAULT_MODEL env: ${envDefault}`);
      return;
    }
    // Fall back to first registered provider
    const first = this.configs.keys().next();
    if (!first.done) {
      this.defaultModelName = first.value;
      logger.info(`Default model set to first registered provider: ${this.defaultModelName}`);
    }
  }

  // ── Public API ─────────────────────────────────────────────────────────────

  /** Get the current default model name. */
  getDefaultModel(): string | null {
    return this.defaultModelName;
  }

  /** Set the default model name. Must be a registered model. */
  setDefaultModel(modelName: string): void {
    if (!this.configs.has(modelName)) {
      const auto = this.autoDetect(modelName);
      if (!auto) {
        throw new Error(`Cannot set default model: '${modelName}' is not registered`);
      }
    }
    this.defaultModelName = modelName;
    logger.info(`Default model updated to: ${modelName}`);
  }

  /**
   * Resolve a model name to its provider config.
   * Tries explicit registry first, then auto-detects from the model name.
   * Falls back to the default model when the name is 'default'.
   * Throws if no provider can be found.
   */
  resolve(modelName: string): ProviderConfig {
    const checkEnabled = (cfg: ProviderConfig | null): ProviderConfig | null => {
      if (cfg && cfg.enabled === false) {
        throw new Error(`Provider for model '${cfg.modelName}' is disabled.`);
      }
      return cfg;
    };

    // Handle 'default' model alias
    if (modelName === 'default') {
      if (this.defaultModelName) {
        const defaultCfg = this.configs.get(this.defaultModelName);
        if (defaultCfg) return checkEnabled(defaultCfg)!;
        const autoDefault = this.autoDetect(this.defaultModelName);
        if (autoDefault) return autoDefault;
      }
      throw new Error(
        `No default model configured. Set DEFAULT_MODEL env var or configure one via Settings.`
      );
    }

    const explicit = this.configs.get(modelName);
    if (explicit) return checkEnabled(explicit)!;

    const auto = this.autoDetect(modelName);
    if (auto) return auto;

    // Last resort: try the default model
    if (this.defaultModelName) {
      logger.warn(
        `Model '${modelName}' not found, falling back to default '${this.defaultModelName}'`
      );
      const fallback = this.configs.get(this.defaultModelName);
      if (fallback) return checkEnabled(fallback)!;
    }

    throw new Error(
      `No provider registered for model '${modelName}'. ` +
        `Register it in ${this.configPath} or via POST /api/providers.`
    );
  }

  register(config: ProviderConfig): void {
    this.configs.set(config.modelName, config);
  }

  /** Returns true if a provider was removed or disabled. */
  unregister(modelName: string): boolean {
    const config = this.configs.get(modelName);
    if (!config) return false;

    if (config.dynamicProviderId) {
      // Dynamic models can be fully removed
      return this.configs.delete(modelName);
    } else {
      // Static models are just disabled
      config.enabled = false;
      return true;
    }
  }

  /** Register multiple models for a specific dynamic provider, cleaning up old ones. */
  registerDynamicModels(providerId: string, models: ProviderConfig[]): void {
    // Remove old models from this dynamic provider
    for (const [name, cfg] of this.configs.entries()) {
      if (cfg.dynamicProviderId === providerId) {
        this.configs.delete(name);
      }
    }
    // Add new ones
    for (const model of models) {
      this.configs.set(model.modelName, { ...model, dynamicProviderId: providerId });
    }
    logger.debug(`Registered ${models.length} model(s) for dynamic provider ${providerId}`);
  }

  /** Remove all models for a specific dynamic provider. */
  unregisterDynamicModels(providerId: string): void {
    let count = 0;
    for (const [name, cfg] of this.configs.entries()) {
      if (cfg.dynamicProviderId === providerId) {
        this.configs.delete(name);
        count++;
      }
    }
    logger.debug(`Removed ${count} model(s) for dynamic provider ${providerId}`);
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
