import type { Request } from 'express';

export type OperatorRole = 'admin' | 'operator' | 'viewer' | 'agent-runner';

export interface OperatorIdentity {
  sub: string;
  email?: string;
  name?: string;
  roles: OperatorRole[];
  authMethod: 'oidc' | 'api-key';
}

/**
 * Interface for pluggable authentication methods.
 */
export interface AuthPlugin {
  readonly name: string;
  /**
   * Authenticate a request and return an operator identity.
   * Returns null if the request does not provide credentials for this plugin.
   * Throws if credentials are provided but invalid.
   */
  authenticate(req: Request): Promise<OperatorIdentity | null>;
}
