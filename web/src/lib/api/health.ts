import { request } from './client';
import type { HealthDetail, CircuitBreakerState } from './types';

export function getHealthDetail(): Promise<HealthDetail> {
  return request<HealthDetail>('/health/detail');
}

export async function getCircuitBreakers(): Promise<CircuitBreakerState[]> {
  const data = await request<{ circuitBreakers: CircuitBreakerState[] }>('/system/circuit-breakers');
  return data.circuitBreakers ?? [];
}

export function resetCircuitBreaker(provider: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/system/circuit-breakers/${encodeURIComponent(provider)}/reset`,
    { method: 'POST' }
  );
}
