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
  return request<ProvidersResponse>('/providers');
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
