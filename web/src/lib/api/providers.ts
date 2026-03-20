import { request } from './client';
import type { ProviderConfig, ProvidersResponse, LLMConfig } from './types';

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
  config: Partial<Pick<ProviderConfig, 'baseUrl' | 'model'> & { apiKey?: string }>,
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/providers/${encodeURIComponent(id)}`,
    {
      method: 'PUT',
      body: JSON.stringify(config),
    },
  );
}

export function testProvider(
  id: string,
): Promise<{ success: boolean; provider: string; response?: string; error?: string }> {
  return request(`/providers/${encodeURIComponent(id)}/test`, {
    method: 'POST',
  });
}

export function setActiveProvider(
  providerId: string,
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
