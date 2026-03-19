import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as agentsApi from '@/lib/api/agents';
import type { AgentManifest } from '@/lib/api/types';

export const agentsKeys = {
  all: ['agents'] as const,
  detail: (name: string) => ['agents', name] as const,
};

export function useAgents() {
  return useQuery({
    queryKey: agentsKeys.all,
    queryFn: agentsApi.listAgents,
  });
}

export function useAgent(name: string) {
  return useQuery({
    queryKey: agentsKeys.detail(name),
    queryFn: () => agentsApi.getAgent(name),
    enabled: name.length > 0,
  });
}

export function useUpdateAgentManifest() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, manifest }: { name: string; manifest: AgentManifest }) =>
      agentsApi.updateAgentManifest(name, manifest),
    onSuccess: (_data, { name }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(name) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useReloadAgents() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: agentsApi.reloadAgents,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}
