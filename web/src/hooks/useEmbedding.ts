import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as embeddingApi from '@/lib/api/embedding';
import { EMBEDDING_PROVIDERS, type EmbeddingProvider, type EmbeddingConfig } from '@/lib/api/embedding';

export { EMBEDDING_PROVIDERS };
export type { EmbeddingProvider, EmbeddingConfig };

export const embeddingKeys = {
  config: ['embedding', 'config'] as const,
  status: ['embedding', 'status'] as const,
  models: ['embedding', 'models'] as const,
  knownModels: ['embedding', 'known-models'] as const,
};

export function useEmbeddingConfig() {
  return useQuery({
    queryKey: embeddingKeys.config,
    queryFn: embeddingApi.getEmbeddingConfig,
  });
}

export function useEmbeddingStatus() {
  return useQuery({
    queryKey: embeddingKeys.status,
    queryFn: embeddingApi.getEmbeddingStatus,
    refetchInterval: 30_000,
  });
}

export function useUpdateEmbeddingConfig() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (config: EmbeddingConfig) => embeddingApi.updateEmbeddingConfig(config),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: embeddingKeys.config });
      void qc.invalidateQueries({ queryKey: embeddingKeys.status });
    },
  });
}

export function useTestEmbeddingConfig() {
  return useMutation({
    mutationFn: (config: EmbeddingConfig) => embeddingApi.testEmbeddingConfig(config),
  });
}

export function useEmbeddingModels(provider?: string, baseUrl?: string) {
  return useQuery({
    queryKey: [...embeddingKeys.models, provider, baseUrl],
    queryFn: () => embeddingApi.getEmbeddingModels(provider, baseUrl),
    enabled: !!provider,
  });
}

export function useKnownEmbeddingModels() {
  return useQuery({
    queryKey: embeddingKeys.knownModels,
    queryFn: embeddingApi.getKnownEmbeddingModels,
    staleTime: Infinity,
  });
}
