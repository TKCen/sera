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
  /** Context window size in tokens. Default: 128000. */
  contextWindow?: number | undefined;
  /** Maximum output tokens per response. Default: 4096. */
  maxTokens?: number | undefined;
  /**
   * Context management strategy when conversation exceeds the high-water mark.
   * - 'summarize': use an LLM to summarize/compact older messages (recommended)
   * - 'sliding-window': drop oldest non-system messages
   * - 'truncate': hard cut-off at the limit
   *
   * Default: 'summarize' if contextCompactionModel is set, otherwise 'sliding-window'.
   */
  contextStrategy?: 'summarize' | 'sliding-window' | 'truncate' | undefined;
  /**
   * High-water mark as a fraction of contextWindow (0.0–1.0).
   * When context exceeds this percentage, the strategy is applied.
   * Default: 0.80 (80%).
   */
  contextHighWaterMark?: number | undefined;
  /**
   * Model name to use for context compaction/summarization.
   * Should be a fast, cheap model (e.g. a local model or haiku-class).
   * When set, enables the 'summarize' strategy.
   * If not set, summarize strategy falls back to sliding-window.
   */
  contextCompactionModel?: string | undefined;
  /**
   * Whether this model supports extended thinking / reasoning.
   * When true, pi-mono separates `reasoning_content` (thinking) from
   * `content` (answer) in the streaming response. Required for Qwen3
   * thinking models, o1/o3 series, DeepSeek-R1, etc.
   *
   * Default: auto-detected from model name (qwen3*, o1*, o3*, deepseek-r1*).
   */
  reasoning?: boolean | undefined;
  /**
   * Model input capabilities, e.g. ['text', 'image'].
   * Default: ['text'].
   */
  input?: string[] | undefined;
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
    // Handle 'default' model alias
    if (modelName === 'default') {
      if (this.defaultModelName) {
        const defaultCfg = this.configs.get(this.defaultModelName);
        if (defaultCfg) return defaultCfg;
        const autoDefault = this.autoDetect(this.defaultModelName);
        if (autoDefault) return autoDefault;
      }
      throw new Error(
        `No default model configured. Set DEFAULT_MODEL env var or configure one via Settings.`
      );
    }

    const explicit = this.configs.get(modelName);
    if (explicit) return explicit;

    const auto = this.autoDetect(modelName);
    if (auto) return auto;

    // Last resort: try the default model
    if (this.defaultModelName) {
      logger.warn(
        `Model '${modelName}' not found, falling back to default '${this.defaultModelName}'`
      );
      const fallback = this.configs.get(this.defaultModelName);
      if (fallback) return fallback;
    }

    throw new Error(
      `No provider registered for model '${modelName}'. ` +
        `Register it in ${this.configPath} or via POST /api/providers.`
    );
  }

  register(config: ProviderConfig): void {
    this.configs.set(config.modelName, config);
  }

  /** Returns true if a provider was removed. */
  unregister(modelName: string): boolean {
    return this.configs.delete(modelName);
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

  /** Determine auth status for a provider config. */
  getAuthStatus(config: ProviderConfig): 'configured' | 'missing' | 'not-required' {
    const LOCAL_PLACEHOLDERS = new Set(['lm-studio', 'ollama', 'none', 'local', '']);

    // Local providers with placeholder keys don't need real auth
    if (config.baseUrl && config.apiKey && LOCAL_PLACEHOLDERS.has(config.apiKey)) {
      return 'not-required';
    }

    // Literal key present
    if (config.apiKey && !LOCAL_PLACEHOLDERS.has(config.apiKey)) {
      return 'configured';
    }

    // Env var or sera-secret reference configured
    if (config.apiKeyEnvVar) {
      if (config.apiKeyEnvVar.startsWith('sera-secret:')) return 'configured';
      if (process.env[config.apiKeyEnvVar]) return 'configured';
    }

    // pi-mono standard env var fallback (OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.)
    if (config.provider) {
      const standardEnvVars: Record<string, string[]> = {
        openai: ['OPENAI_API_KEY'],
        anthropic: ['ANTHROPIC_API_KEY'],
        google: ['GOOGLE_API_KEY', 'GEMINI_API_KEY'],
        groq: ['GROQ_API_KEY'],
        mistral: ['MISTRAL_API_KEY'],
      };
      const envVars = standardEnvVars[config.provider];
      if (envVars?.some((v) => process.env[v])) {
        return 'configured';
      }
    }

    // Local providers (no cloud API key needed)
    const LOCAL_PROVIDERS = new Set(['lmstudio', 'ollama', 'vllm', 'local', 'default']);
    if (config.baseUrl && config.provider && LOCAL_PROVIDERS.has(config.provider)) {
      return 'not-required';
    }

    return 'missing';
  }

  /** List providers with auth status enrichment (for the UI). */
  listWithStatus(): (ProviderConfig & { authStatus: 'configured' | 'missing' | 'not-required' })[] {
    return this.list().map((cfg) => ({
      ...cfg,
      // Strip literal API keys from the response
      apiKey: cfg.apiKey ? '***' : undefined,
      authStatus: this.getAuthStatus(cfg),
    }));
  }

  /**
   * Persist provider configs to disk.
   * API keys that look like real secrets (not local placeholders) are
   * stripped from the JSON file and stored in the encrypted secrets table
   * instead, referenced via `apiKeyEnvVar: "sera-secret:provider-key:<provider>"`.
   */
  async save(): Promise<void> {
    const LOCAL_PLACEHOLDERS = new Set(['lm-studio', 'ollama', 'none', 'local', '']);
    const providers: ProviderConfig[] = [];

    for (const cfg of this.configs.values()) {
      const clone = { ...cfg };

      // Move real API keys to secrets store, keep only the reference in the config file
      if (clone.apiKey && !LOCAL_PLACEHOLDERS.has(clone.apiKey)) {
        const secretName = `provider-key:${clone.provider ?? clone.modelName}`;
        try {
          await ProviderRegistry.storeApiKeySecret(secretName, clone.apiKey);
          clone.apiKeyEnvVar = `sera-secret:${secretName}`;
          delete clone.apiKey;
        } catch {
          // Secrets store not available (no SECRETS_MASTER_KEY) — keep literal key
          // but log a warning
          logger.warn(
            `Cannot store API key for ${clone.modelName} in secrets DB (SECRETS_MASTER_KEY not set). Key saved in plaintext.`
          );
        }
      }

      providers.push(clone);
    }

    const data: ConfigFile = { providers };
    await fs.promises.writeFile(this.configPath, JSON.stringify(data, null, 2) + '\n', 'utf-8');
    logger.debug(`Saved ${this.configs.size} provider(s) to ${this.configPath}`);
  }

  /**
   * Resolve the effective API key for a provider config.
   * Resolution order: literal apiKey → apiKeyEnvVar (env or secrets) → standard env var.
   */
  async resolveApiKey(config: ProviderConfig): Promise<string | undefined> {
    // 1. Literal key
    if (config.apiKey) return config.apiKey;

    // 2. Env var or sera-secret reference
    if (config.apiKeyEnvVar) {
      if (config.apiKeyEnvVar.startsWith('sera-secret:')) {
        const secretName = config.apiKeyEnvVar.slice('sera-secret:'.length);
        const val = await ProviderRegistry.getApiKeySecret(secretName);
        if (val) return val;
      }
      const envVal = process.env[config.apiKeyEnvVar];
      if (envVal) return envVal;
    }

    // 3. Standard provider env vars
    if (config.provider) {
      const standardEnvVars: Record<string, string[]> = {
        openai: ['OPENAI_API_KEY'],
        anthropic: ['ANTHROPIC_API_KEY'],
        google: ['GOOGLE_API_KEY', 'GEMINI_API_KEY'],
        groq: ['GROQ_API_KEY'],
        mistral: ['MISTRAL_API_KEY'],
      };
      const envVars = standardEnvVars[config.provider];
      if (envVars) {
        for (const v of envVars) {
          if (process.env[v]) return process.env[v];
        }
      }
    }

    return undefined;
  }

  /**
   * On startup, hydrate in-memory configs with API keys from the secrets store.
   * This runs after loadSync() so configs with `sera-secret:` references get their keys resolved.
   */
  async hydrateSecrets(): Promise<void> {
    let hydrated = 0;
    for (const [name, cfg] of this.configs.entries()) {
      if (cfg.apiKeyEnvVar?.startsWith('sera-secret:')) {
        const secretName = cfg.apiKeyEnvVar.slice('sera-secret:'.length);
        try {
          const val = await ProviderRegistry.getApiKeySecret(secretName);
          if (val) {
            cfg.apiKey = val;
            hydrated++;
          }
        } catch {
          logger.warn(`Failed to hydrate secret for provider ${name}`);
        }
      }
    }
    if (hydrated > 0) {
      logger.info(`Hydrated ${hydrated} provider API key(s) from secrets store`);
    }
  }

  /**
   * Ingest API keys discovered in environment variables into the encrypted
   * secrets store.  This makes env vars a one-time ingestion path — after
   * the first startup the key lives in the DB and survives container rebuilds
   * without needing the env var again.
   *
   * Only ingests keys that are NOT already stored (doesn't overwrite).
   * Runs after hydrateSecrets() so we can check what's already persisted.
   */
  async ingestEnvKeys(): Promise<void> {
    const STANDARD_ENV_VARS: Record<string, string[]> = {
      openai: ['OPENAI_API_KEY'],
      anthropic: ['ANTHROPIC_API_KEY'],
      google: ['GOOGLE_API_KEY', 'GEMINI_API_KEY'],
      groq: ['GROQ_API_KEY'],
      mistral: ['MISTRAL_API_KEY'],
    };

    let ingested = 0;

    for (const [provider, envVars] of Object.entries(STANDARD_ENV_VARS)) {
      // Find the first env var that has a value
      let envValue: string | undefined;
      for (const v of envVars) {
        if (process.env[v]) {
          envValue = process.env[v];
          break;
        }
      }
      if (!envValue) continue;

      // Check if we already have this key in the secrets store
      const secretName = `provider-key:${provider}`;
      try {
        const existing = await ProviderRegistry.getApiKeySecret(secretName);
        if (existing) continue; // Already stored — don't overwrite

        await ProviderRegistry.storeApiKeySecret(secretName, envValue);
        ingested++;
        logger.info(`Ingested ${provider} API key from env into secrets store`);
      } catch {
        // Secrets store not available (no SECRETS_MASTER_KEY) — skip silently
      }
    }

    if (ingested > 0) {
      logger.info(`Ingested ${ingested} API key(s) from env vars into secrets store`);
    }
  }

  /**
   * Update configuration for an existing provider model.
   * Merges overrides into the existing config and persists to file.
   */
  updateConfig(
    modelName: string,
    overrides: {
      contextWindow?: number;
      maxTokens?: number;
      contextStrategy?: 'summarize' | 'sliding-window' | 'truncate';
      contextHighWaterMark?: number;
      contextCompactionModel?: string;
      reasoning?: boolean;
      input?: string[];
      description?: string;
    }
  ): void {
    const existing = this.configs.get(modelName);
    if (!existing) {
      throw new Error(`Model "${modelName}" not found in provider registry`);
    }

    // Merge only defined fields (exactOptionalPropertyTypes safe)
    if (overrides.contextWindow !== undefined) existing.contextWindow = overrides.contextWindow;
    if (overrides.maxTokens !== undefined) existing.maxTokens = overrides.maxTokens;
    if (overrides.contextStrategy !== undefined)
      existing.contextStrategy = overrides.contextStrategy;
    if (overrides.contextHighWaterMark !== undefined)
      existing.contextHighWaterMark = overrides.contextHighWaterMark;
    if (overrides.contextCompactionModel !== undefined)
      existing.contextCompactionModel = overrides.contextCompactionModel;
    if (overrides.reasoning !== undefined) existing.reasoning = overrides.reasoning;
    if (overrides.input !== undefined) existing.input = overrides.input;
    if (overrides.description !== undefined) existing.description = overrides.description;

    logger.info(`Updated config for model ${modelName}:`, overrides);
  }

  // ── Secret helpers (static to avoid circular deps) ──────────────────────────

  private static async storeApiKeySecret(name: string, value: string): Promise<void> {
    // Lazy import to avoid circular dependency with SecretsManager
    const { SecretsManager } = await import('../secrets/secrets-manager.js');
    const mgr = SecretsManager.getInstance();
    await mgr.set(
      name,
      value,
      { operator: { sub: 'system', roles: ['admin'] } },
      { description: `Provider API key: ${name}`, allowedAgents: ['*'], tags: ['provider-key'] }
    );
  }

  private static async getApiKeySecret(name: string): Promise<string | null> {
    const { SecretsManager } = await import('../secrets/secrets-manager.js');
    const mgr = SecretsManager.getInstance();
    return mgr.get(name, { operator: { sub: 'system', roles: ['admin'] } });
  }
}
