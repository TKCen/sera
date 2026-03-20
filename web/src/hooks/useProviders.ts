import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as providersApi from '@/lib/api/providers';
import type { LLMConfig, ProviderConfig } from '@/lib/api/types';
import type { NewProviderPayload } from '@/lib/api/providers';

export const providersKeys = {
  all: ['providers'] as const,
  llmConfig: ['providers', 'llm-config'] as const,
};

export function useProviders() {
  return useQuery({
    queryKey: providersKeys.all,
    queryFn: providersApi.getProviders,
  });
}

export function useLLMConfig() {
  return useQuery({
    queryKey: providersKeys.llmConfig,
    queryFn: providersApi.getLLMConfig,
  });
}

export function useUpdateProvider() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      config,
    }: {
      id: string;
      config: Partial<Pick<ProviderConfig, 'baseUrl' | 'model'> & { apiKey?: string }>;
    }) => providersApi.updateProvider(id, config),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useSetActiveProvider() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (providerId: string) => providersApi.setActiveProvider(providerId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useUpdateLLMConfig() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (config: LLMConfig) => providersApi.updateLLMConfig(config),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: providersKeys.llmConfig });
    },
  });
}

export function useCreateProvider() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (payload: NewProviderPayload) => providersApi.createProvider(payload),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useDeleteProvider() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => providersApi.deleteProvider(name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}
