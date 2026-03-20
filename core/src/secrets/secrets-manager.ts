import type {
  SecretsProvider,
  SecretAccessContext,
  SecretMetadata,
  SecretFilter,
} from './interfaces.js';
import { PostgresSecretsProvider } from './postgres-secrets-provider.js';
import { Logger } from '../lib/logger.js';
import { AuditService } from '../audit/AuditService.js';

const logger = new Logger('SecretsManager');

export class SecretsManager {
  private static instance: SecretsManager;
  private provider: SecretsProvider;

  private constructor() {
    this.provider = new PostgresSecretsProvider();
  }

  public static getInstance(): SecretsManager {
    if (!SecretsManager.instance) {
      SecretsManager.instance = new SecretsManager();
    }
    return SecretsManager.instance;
  }

  setProvider(provider: SecretsProvider) {
    this.provider = provider;
  }

  async get(name: string, context: SecretAccessContext): Promise<string | null> {
    const val = await this.provider.get(name, context);

    await AuditService.getInstance()
      .record({
        actorType: context.agentId ? 'agent' : 'operator',
        actorId: context.agentId || context.operator?.sub || 'unknown',
        actingContext: null,
        eventType: 'secret.accessed',
        payload: { secretName: name },
      })
      .catch((err) => logger.error('Audit record failed:', err));

    return val;
  }

  async set(
    name: string,
    value: string,
    context: SecretAccessContext,
    metadata?: Partial<SecretMetadata>
  ): Promise<void> {
    // context is not strictly needed for set if we trust the router, but good for auditing
    return this.provider.set(name, value, metadata);
  }

  async delete(name: string, context: SecretAccessContext): Promise<boolean> {
    return this.provider.delete(name, context);
  }

  async list(filter: SecretFilter, context: SecretAccessContext): Promise<SecretMetadata[]> {
    return this.provider.list(filter, context);
  }

  async healthCheck(): Promise<boolean> {
    return this.provider.healthCheck();
  }
}
