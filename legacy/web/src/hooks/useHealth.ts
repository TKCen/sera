import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as healthApi from '@/lib/api/health';

export const healthKeys = {
  detail: ['health', 'detail'] as const,
  circuitBreakers: ['health', 'circuit-breakers'] as const,
};

export function useHealthDetail() {
  return useQuery({
    queryKey: healthKeys.detail,
    queryFn: healthApi.getHealthDetail,
    refetchInterval: 30_000,
  });
}

export function useCircuitBreakers() {
  return useQuery({
    queryKey: healthKeys.circuitBreakers,
    queryFn: healthApi.getCircuitBreakers,
    refetchInterval: 30_000,
  });
}

export function useResetCircuitBreaker() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (provider: string) => healthApi.resetCircuitBreaker(provider),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: healthKeys.circuitBreakers });
    },
  });
}
