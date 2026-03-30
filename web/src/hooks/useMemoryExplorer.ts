import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getMemoryOverview,
  getRecentBlocks,
  searchMemoryBlocks,
  getExplorerGraph,
  getAgentBlock,
  getAgentBacklinks,
  getAgentBlocks,
  promoteBlock,
} from '@/lib/api/memory';

export const memoryExplorerKeys = {
  overview: ['memory-explorer', 'overview'] as const,
  recent: (limit?: number) => ['memory-explorer', 'recent', limit] as const,
  search: (query: string) => ['memory-explorer', 'search', query] as const,
  graph: ['memory-explorer', 'graph'] as const,
  block: (agentId: string, blockId: string) =>
    ['memory-explorer', 'block', agentId, blockId] as const,
  backlinks: (agentId: string, blockId: string) =>
    ['memory-explorer', 'backlinks', agentId, blockId] as const,
  agentBlocks: (agentId: string) => ['memory-explorer', 'agent-blocks', agentId] as const,
};

export function useMemoryOverview() {
  return useQuery({
    queryKey: memoryExplorerKeys.overview,
    queryFn: getMemoryOverview,
  });
}

export function useRecentBlocks(limit = 20) {
  return useQuery({
    queryKey: memoryExplorerKeys.recent(limit),
    queryFn: () => getRecentBlocks(limit),
  });
}

export function useMemorySearch(query: string) {
  return useQuery({
    queryKey: memoryExplorerKeys.search(query),
    queryFn: () => searchMemoryBlocks(query),
    enabled: query.length >= 2,
  });
}

export function useExplorerGraph() {
  return useQuery({
    queryKey: memoryExplorerKeys.graph,
    queryFn: getExplorerGraph,
  });
}

export function useBlockDetail(agentId: string, blockId: string) {
  return useQuery({
    queryKey: memoryExplorerKeys.block(agentId, blockId),
    queryFn: () => getAgentBlock(agentId, blockId),
    enabled: agentId.length > 0 && blockId.length > 0,
  });
}

export function useBlockBacklinks(agentId: string, blockId: string) {
  return useQuery({
    queryKey: memoryExplorerKeys.backlinks(agentId, blockId),
    queryFn: () => getAgentBacklinks(agentId, blockId),
    enabled: agentId.length > 0 && blockId.length > 0,
  });
}

export function useAgentBlockList(agentId: string) {
  return useQuery({
    queryKey: memoryExplorerKeys.agentBlocks(agentId),
    queryFn: () => getAgentBlocks(agentId),
    enabled: agentId.length > 0,
  });
}

export function usePromoteBlock() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      agentId,
      blockId,
      targetScope,
      circleId,
    }: {
      agentId: string;
      blockId: string;
      targetScope: 'circle' | 'global';
      circleId?: string;
    }) => promoteBlock(agentId, blockId, targetScope, circleId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['memory-explorer'] });
    },
  });
}
