import { request } from './client';

export type EmbeddingProvider = 'ollama' | 'openai' | 'lm-studio' | 'openai-compatible';

export interface EmbeddingConfig {
  provider: EmbeddingProvider;
  model: string;
  baseUrl: string;
  apiKey?: string;
  apiKeyEnvVar?: string;
  dimension: number;
}

export interface EmbeddingStatus {
  available: boolean;
  configured: boolean;
  provider: string;
  model: string;
  dimension: number;
  baseUrl: string;
}

export interface EmbeddingTestResult {
  ok: boolean;
  latencyMs: number;
  dimension?: number;
  error?: string;
}

export interface EmbeddingModel {
  id: string;
  dimension?: number;
  description?: string;
}

export interface KnownEmbeddingModel {
  provider: EmbeddingProvider;
  dimension: number;
  description: string;
}

export const EMBEDDING_PROVIDERS: { value: EmbeddingProvider; label: string }[] = [
  { value: 'ollama', label: 'Ollama' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'lm-studio', label: 'LM Studio' },
  { value: 'openai-compatible', label: 'OpenAI Compatible' },
];

export function getEmbeddingConfig(): Promise<EmbeddingConfig> {
  return request<EmbeddingConfig>('/embedding/config');
}

export function updateEmbeddingConfig(config: EmbeddingConfig): Promise<{
  config: EmbeddingConfig;
  testResult: EmbeddingTestResult;
  dimensionChanged?: boolean;
  warning?: string;
}> {
  return request('/embedding/config', { method: 'PUT', body: JSON.stringify(config) });
}

export function testEmbeddingConfig(config: EmbeddingConfig): Promise<EmbeddingTestResult> {
  return request<EmbeddingTestResult>('/embedding/test', {
    method: 'POST',
    body: JSON.stringify(config),
  });
}

export function getEmbeddingModels(
  provider?: string,
  baseUrl?: string
): Promise<{ models: EmbeddingModel[] }> {
  const params = new URLSearchParams();
  if (provider) params.set('provider', provider);
  if (baseUrl) params.set('baseUrl', baseUrl);
  const qs = params.toString();
  return request(`/embedding/models${qs ? `?${qs}` : ''}`);
}

export function getEmbeddingStatus(): Promise<EmbeddingStatus> {
  return request<EmbeddingStatus>('/embedding/status');
}

export function getKnownEmbeddingModels(): Promise<Record<string, KnownEmbeddingModel>> {
  return request('/embedding/known-models');
}
