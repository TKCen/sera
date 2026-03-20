import { request } from './client';
import type { HealthDetail, CircuitBreakerState } from './types';

export function getHealthDetail(): Promise<HealthDetail> {
  return request<HealthDetail>('/health/detail');
}

export function getCircuitBreakers(): Promise<CircuitBreakerState[]> {
  return request<CircuitBreakerState[]>('/system/circuit-breakers');
}

export function resetCircuitBreaker(provider: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/system/circuit-breakers/${encodeURIComponent(provider)}/reset`,
    { method: 'POST' },
  );
}
