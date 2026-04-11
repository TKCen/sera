import { request } from './client';

export interface IssueDelegationParams {
  agentId: string;
  service: string;
  permissions: string[];
  resourceConstraints?: Record<string, string[]>;
  credentialSecretName: string;
  grantType?: 'one-time' | 'session' | 'persistent';
  expiresAt?: string;
  instanceScoped?: boolean;
  instanceId?: string;
}

export function issueDelegation(params: IssueDelegationParams) {
  return request<{ id: string; signedToken: string }>('/delegation/issue', {
    method: 'POST',
    body: JSON.stringify(params),
  });
}
