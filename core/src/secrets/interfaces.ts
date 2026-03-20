export interface SecretAccessContext {
  agentId?: string;
  agentName?: string;
  operator?: {
    sub: string;
    roles: string[];
  };
}

export interface SecretMetadata {
  id: string;
  name: string;
  description?: string;
  allowedAgents: string[];
  allowedCircles: string[];
  tags: string[];
  exposure: 'per-call' | 'agent-env';
  createdAt: Date;
  updatedAt: Date;
  rotatedAt?: Date;
  expiresAt?: Date;
}

export interface SecretFilter {
  tags?: string[];
  agentId?: string;
}

/**
 * Pluggable secrets provider interface.
 */
export interface SecretsProvider {
  readonly id: string;

  /**
   * Retrieve a secret value.
   * Enforces access control internally (optional, but recommended).
   */
  get(name: string, context: SecretAccessContext): Promise<string | null>;

  /**
   * Set/update a secret value.
   */
  set(name: string, value: string, metadata?: Partial<SecretMetadata>): Promise<void>;

  /**
   * Delete a secret.
   */
  delete(name: string, context: SecretAccessContext): Promise<boolean>;

  /**
   * List metadata for all secrets matching the filter.
   */
  list(filter: SecretFilter, context: SecretAccessContext): Promise<SecretMetadata[]>;

  /**
   * Perform a health check on the secrets backend.
   */
  healthCheck(): Promise<boolean>;
}
