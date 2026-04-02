import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as memoryApi from '@/lib/api/memory';

export const memoryKeys = {
  all: (agentId: string) => ['memory', agentId] as const,
  blocks: (agentId: string, params?: any) => ['memory', agentId, 'blocks', params] as const,
  stats: (agentId: string) => ['memory', agentId, 'stats'] as const,
  links: (agentId: string, entryId?: string) => ['memory', agentId, 'links', entryId] as const,
};

export function useAgentBlocks(agentId: string, params?: any) {
  return useQuery({
    queryKey: memoryKeys.blocks(agentId, params),
    queryFn: () => memoryApi.getAgentBlocks(agentId, params),
    enabled: !!agentId,
  });
}

export function useAgentStats(agentId: string) {
  return useQuery({
    queryKey: memoryKeys.stats(agentId),
    queryFn: () => memoryApi.getAgentStats(agentId),
    enabled: !!agentId,
  });
}

export function useAgentLinks(agentId: string, entryId?: string) {
  return useQuery({
    queryKey: memoryKeys.links(agentId, entryId),
    queryFn: () => memoryApi.getAgentLinks(agentId, entryId),
    enabled: !!agentId,
  });
}

export function useAddMemoryEntry() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ type, entry }: { type: string; entry: any }) =>
      memoryApi.addMemoryEntry(type, entry),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['memory'] });
      void qc.invalidateQueries({ queryKey: ['agents'] });
    },
  });
}
