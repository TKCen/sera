import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as memoryApi from '@/lib/api/memory';
import type { MemoryEntry } from '@/lib/api/types';

export const memoryKeys = {
  blocks: ['memory', 'blocks'] as const,
  block: (type: string) => ['memory', 'blocks', type] as const,
  entry: (id: string) => ['memory', 'entries', id] as const,
  graph: ['memory', 'graph'] as const,
  search: (query: string) => ['memory', 'search', query] as const,
};

export function useMemoryBlocks() {
  return useQuery({
    queryKey: memoryKeys.blocks,
    queryFn: memoryApi.getMemoryBlocks,
  });
}

export function useMemoryGraph() {
  return useQuery({
    queryKey: memoryKeys.graph,
    queryFn: memoryApi.getMemoryGraph,
  });
}

export function useSearchMemory(query: string) {
  return useQuery({
    queryKey: memoryKeys.search(query),
    queryFn: () => memoryApi.searchMemory(query),
    enabled: query.length >= 2,
  });
}

export function useAddMemoryEntry() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      type,
      entry,
    }: {
      type: string;
      entry: Pick<MemoryEntry, 'title' | 'content'> & {
        refs?: string[];
        tags?: string[];
        source?: string;
      };
    }) => memoryApi.addMemoryEntry(type, entry),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: memoryKeys.blocks });
      void qc.invalidateQueries({ queryKey: memoryKeys.graph });
    },
  });
}

export function useDeleteMemoryEntry() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => memoryApi.deleteMemoryEntry(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: memoryKeys.blocks });
      void qc.invalidateQueries({ queryKey: memoryKeys.graph });
    },
  });
}
