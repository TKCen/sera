import fs from 'fs';
import { Logger } from '../lib/logger.js';
import type {
  DynamicProviderConfig,
  DynamicProviderStatus,
  ProviderConfig,
  ProviderRegistry,
} from './ProviderRegistry.js';

const logger = new Logger('DynamicProviderManager');

interface DynamicProvidersFile {
  dynamicProviders: DynamicProviderConfig[];
}

export class DynamicProviderManager {
  private providers = new Map<string, DynamicProviderConfig>();
  private statuses = new Map<string, DynamicProviderStatus>();
  private timers = new Map<string, NodeJS.Timeout>();
  private readonly configPath: string;
  private readonly registry: ProviderRegistry;

  constructor(registry: ProviderRegistry, configPath: string) {
    this.registry = registry;
    this.configPath = configPath;
    this.loadSync();
  }

  private loadSync(): void {
    if (!fs.existsSync(this.configPath)) {
      logger.info(`No dynamic providers config at ${this.configPath}`);
      return;
    }
    try {
      const raw = fs.readFileSync(this.configPath, 'utf-8');
      const data = JSON.parse(raw) as DynamicProvidersFile;
      for (const cfg of data.dynamicProviders ?? []) {
        this.providers.set(cfg.id, cfg);
      }
      logger.info(`Loaded ${this.providers.size} dynamic provider(s) from ${this.configPath}`);
    } catch (err: unknown) {
      logger.error(`Failed to load dynamic providers: ${(err as Error).message}`);
    }
  }

  private async save(): Promise<void> {
    const LOCAL_PLACEHOLDERS = new Set(['lm-studio', 'ollama', 'none', 'local', '']);
    const toSave: DynamicProviderConfig[] = [];

    for (const cfg of this.providers.values()) {
      const clone = { ...cfg };

      // Move real API keys to secrets store, keep only the reference in the config file.
      // Skip keys already stored as sera-secret: references.
      if (
        clone.apiKey &&
        !LOCAL_PLACEHOLDERS.has(clone.apiKey) &&
        !clone.apiKey.startsWith('sera-secret:')
      ) {
        const secretName = `dynamic-provider-key:${clone.id}`;
        try {
          await DynamicProviderManager.storeApiKeySecret(secretName, clone.apiKey);
          clone.apiKey = `sera-secret:${secretName}`;
        } catch {
          // Secrets store not available (no SECRETS_MASTER_KEY) — keep literal key
          logger.warn(
            `Cannot store API key for dynamic provider ${clone.id} in secrets DB (SECRETS_MASTER_KEY not set). Key saved in plaintext.`
          );
        }
      }

      toSave.push(clone);
    }

    const data: DynamicProvidersFile = { dynamicProviders: toSave };
    await fs.promises.writeFile(this.configPath, JSON.stringify(data, null, 2) + '\n', 'utf-8');
  }

  private static async storeApiKeySecret(name: string, value: string): Promise<void> {
    const { SecretsManager } = await import('../secrets/secrets-manager.js');
    const mgr = SecretsManager.getInstance();
    await mgr.set(
      name,
      value,
      { operator: { sub: 'system', roles: ['admin'] } },
      {
        description: `Dynamic provider API key: ${name}`,
        allowedAgents: ['*'],
        tags: ['provider-key'],
      }
    );
  }

  private static async getApiKeySecret(name: string): Promise<string | null> {
    const { SecretsManager } = await import('../secrets/secrets-manager.js');
    const mgr = SecretsManager.getInstance();
    return mgr.get(name, { operator: { sub: 'system', roles: ['admin'] } });
  }

  async start(): Promise<void> {
    for (const provider of this.providers.values()) {
      if (provider.enabled) {
        this.scheduleCheck(provider);
      }
    }
  }

  stop(): void {
    for (const timer of this.timers.values()) {
      clearInterval(timer);
    }
    this.timers.clear();
  }

  private scheduleCheck(provider: DynamicProviderConfig): void {
    // Run immediately
    this.checkProvider(provider).catch((err) => {
      logger.error(`Initial check failed for ${provider.id}: ${err.message}`);
    });

    // Schedule regular checks
    const timer = setInterval(() => {
      this.checkProvider(provider).catch((err) => {
        logger.error(`Scheduled check failed for ${provider.id}: ${err.message}`);
      });
    }, provider.intervalMs || 60_000);

    this.timers.set(provider.id, timer);
  }

  async testConnection(
    baseUrl: string,
    apiKey?: string
  ): Promise<{ success: boolean; models: string[]; error?: string }> {
    try {
      // LM Studio / OpenAI compatible /v1/models
      const url = baseUrl.endsWith('/') ? `${baseUrl}models` : `${baseUrl}/models`;
      const headers: Record<string, string> = {};
      if (apiKey) {
        headers['Authorization'] = `Bearer ${apiKey}`;
      }

      const res = await fetch(url, { headers });
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}: ${res.statusText}`);
      }

      const data = (await res.json()) as { data: Array<{ id: string }> };
      const models = data.data.map((m) => m.id);
      return { success: true, models };
    } catch (err: unknown) {
      return { success: false, models: [], error: (err as Error).message };
    }
  }

  private async checkProvider(provider: DynamicProviderConfig): Promise<void> {
    logger.debug(`Checking dynamic provider ${provider.id} (${provider.baseUrl})`);

    // Resolve sera-secret: references before using the key for HTTP connections
    let resolvedApiKey = provider.apiKey;
    if (resolvedApiKey?.startsWith('sera-secret:')) {
      const secretName = resolvedApiKey.slice('sera-secret:'.length);
      try {
        resolvedApiKey = (await DynamicProviderManager.getApiKeySecret(secretName)) ?? undefined;
      } catch {
        logger.warn(`Failed to resolve secret for dynamic provider ${provider.id}`);
        resolvedApiKey = undefined;
      }
    }

    const result = await this.testConnection(provider.baseUrl, resolvedApiKey);

    const status: DynamicProviderStatus = {
      id: provider.id,
      lastCheck: new Date().toISOString(),
      status: result.success ? 'ok' : 'error',
      error: result.error,
      discoveredModels: result.models,
    };
    this.statuses.set(provider.id, status);

    if (result.success) {
      const modelConfigs: ProviderConfig[] = result.models.map((modelId) => ({
        // We use a prefix to identify models from this provider
        modelName: `dp-${provider.id}-${modelId}`,
        api: 'openai-completions',
        provider: 'lmstudio', // Default for now
        baseUrl: provider.baseUrl,
        // Use the in-memory provider config's apiKey (may be a sera-secret: ref).
        // LlmRouter.resolveApiKey handles sera-secret: resolution via hydrateSecrets().
        // Fall back to the placeholder so LM Studio's auth header is always sent.
        apiKey: provider.apiKey || 'lm-studio',
        description: `Discovered from ${provider.name} (${modelId})`,
      }));
      this.registry.registerDynamicModels(provider.id, modelConfigs);
    } else {
      logger.warn(`Failed to discover models from ${provider.id}: ${result.error}`);
    }
  }

  // ── Public API ─────────────────────────────────────────────────────────────

  async addProvider(config: DynamicProviderConfig): Promise<void> {
    this.providers.set(config.id, config);
    await this.save();
    if (config.enabled) {
      this.scheduleCheck(config);
    }
  }

  async removeProvider(id: string): Promise<void> {
    const timer = this.timers.get(id);
    if (timer) {
      clearInterval(timer);
      this.timers.delete(id);
    }
    this.providers.delete(id);
    this.statuses.delete(id);
    this.registry.unregisterDynamicModels(id);
    await this.save();
  }

  /** Returns dynamic provider configs with API keys redacted for API responses. */
  listProviders(): (Omit<DynamicProviderConfig, 'apiKey'> & { apiKey?: string })[] {
    return [...this.providers.values()].map((cfg) => {
      const { apiKey: _apiKey, ...rest } = cfg;
      // Redact literal keys; show whether a key is configured without revealing the value
      return cfg.apiKey ? { ...rest, apiKey: '***' } : { ...rest };
    });
  }

  getStatuses(): DynamicProviderStatus[] {
    return [...this.statuses.values()];
  }
}
