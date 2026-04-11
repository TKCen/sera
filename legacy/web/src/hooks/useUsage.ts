import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as usageApi from '@/lib/api/usage';

export const usageKeys = {
  all: (params: object) => ['usage', params] as const,
  budget: (agentName: string) => ['usage', 'budget', agentName] as const,
};

export function useUsage(params: { groupBy?: 'agent' | 'model'; from?: string; to?: string }) {
  return useQuery({
    queryKey: usageKeys.all(params),
    queryFn: () => usageApi.getUsage(params),
    refetchInterval: 60_000,
  });
}

export function useAgentBudget(agentName: string) {
  return useQuery({
    queryKey: usageKeys.budget(agentName),
    queryFn: () => usageApi.getAgentBudget(agentName),
    enabled: agentName.length > 0,
    refetchInterval: 30_000,
  });
}

export function usePatchAgentBudget(agentName: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (budget: {
      maxLlmTokensPerHour?: number | null;
      maxLlmTokensPerDay?: number | null;
    }) => usageApi.patchAgentBudget(agentName, budget),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: usageKeys.budget(agentName) });
    },
  });
}

export function useResetAgentBudget(agentName: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => usageApi.resetAgentBudget(agentName),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: usageKeys.budget(agentName) });
    },
  });
}
