import { request } from './client';
import type { ProvidersResponse, LLMConfig } from './types';

export interface NewProviderPayload {
  name: string;
  type: 'local' | 'cloud';
  baseUrl?: string;
  apiKey?: string;
  modelId: string;
}

export function createProvider(payload: NewProviderPayload): Promise<{ success: boolean }> {
  return request<{ success: boolean }>('/providers', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export function deleteProvider(name: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/providers/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
}

export function getProviders(): Promise<ProvidersResponse> {
  return request<ProvidersResponse>('/providers/list');
}

export function updateProvider(
  id: string,
  config: Partial<{ baseUrl?: string; model?: string; apiKey?: string }>
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/providers/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify(config),
  });
}

export function testProvider(
  id: string
): Promise<{ success: boolean; provider: string; response?: string; error?: string }> {
  return request(`/providers/${encodeURIComponent(id)}/test`, {
    method: 'POST',
  });
}

export function setActiveProvider(
  providerId: string
): Promise<{ success: boolean; activeProvider: string }> {
  return request('/providers/active', {
    method: 'POST',
    body: JSON.stringify({ providerId }),
  });
}

export function getLLMConfig(): Promise<LLMConfig> {
  return request<LLMConfig>('/config/llm');
}

export function updateLLMConfig(config: LLMConfig): Promise<{ success: boolean }> {
  return request<{ success: boolean }>('/config/llm', {
    method: 'POST',
    body: JSON.stringify(config),
  });
}

export function testLLMConfig(): Promise<{
  success: boolean;
  model?: string;
  response?: string;
  error?: string;
}> {
  return request('/config/llm/test', { method: 'POST' });
}

// ── Dynamic Providers ──────────────────────────────────────────────────────

export interface DynamicProviderConfig {
  id: string;
  name: string;
  type: 'lm-studio';
  baseUrl: string;
  apiKey?: string;
  enabled: boolean;
  intervalMs: number;
  description?: string;
}

export interface DynamicProviderStatus {
  id: string;
  lastCheck?: string;
  status: 'ok' | 'error';
  error?: string;
  discoveredModels: string[];
}

export function getDynamicProviders(): Promise<{ dynamicProviders: DynamicProviderConfig[] }> {
  return request<{ dynamicProviders: DynamicProviderConfig[] }>('/providers/dynamic');
}

export function getDynamicProviderStatuses(): Promise<{ statuses: DynamicProviderStatus[] }> {
  return request<{ statuses: DynamicProviderStatus[] }>('/providers/dynamic/statuses');
}

export function addDynamicProvider(config: DynamicProviderConfig): Promise<DynamicProviderConfig> {
  return request<DynamicProviderConfig>('/providers/dynamic', {
    method: 'POST',
    body: JSON.stringify(config),
  });
}

export function removeDynamicProvider(id: string): Promise<void> {
  return request<void>(`/providers/dynamic/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function testDynamicConnection(
  baseUrl: string,
  apiKey?: string
): Promise<{ success: boolean; models: string[]; error?: string }> {
  return request<{ success: boolean; models: string[]; error?: string }>(
    '/providers/dynamic/test',
    {
      method: 'POST',
      body: JSON.stringify({ baseUrl, apiKey }),
    }
  );
}

// ── Cloud Provider Templates & Discovery ─────────────────────────────────────

export interface ProviderTemplate {
  provider: string;
  displayName: string;
  api: string;
  models: string[];
  baseUrl?: string;
  apiKeyEnvVar: string;
  description: string;
  supportsDiscovery?: boolean;
}

export function getProviderTemplates(): Promise<{ templates: ProviderTemplate[] }> {
  return request<{ templates: ProviderTemplate[] }>('/providers/templates');
}

export function discoverModels(modelName: string): Promise<{ provider: string; models: string[] }> {
  return request<{ provider: string; models: string[] }>(
    `/providers/${encodeURIComponent(modelName)}/discover`
  );
}

export function getHealthAll(): Promise<{
  health: Record<
    string,
    { provider?: string; reachable: boolean; latencyMs: number; error?: string }
  >;
}> {
  return request('/providers/health-all');
}

export interface AddProviderPayload {
  modelName: string;
  api: string;
  provider?: string;
  baseUrl?: string;
  apiKey?: string;
  apiKeyEnvVar?: string;
  description?: string;
}

export function addProvider(
  payload: AddProviderPayload
): Promise<{ modelName: string; result: { modelName: string; api: string } }> {
  return request('/providers', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

// ── Default Model ────────────────────────────────────────────────────────────

export function getDefaultModel(): Promise<{ defaultModel: string | null }> {
  return request<{ defaultModel: string | null }>('/providers/default-model');
}

export function setDefaultModel(
  modelName: string
): Promise<{ success: boolean; defaultModel: string }> {
  return request<{ success: boolean; defaultModel: string }>('/providers/default-model', {
    method: 'PUT',
    body: JSON.stringify({ modelName }),
  });
}
