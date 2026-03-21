import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getProviders,
  updateProvider,
  setActiveProvider,
  getLLMConfig,
  updateLLMConfig,
  getDynamicProviders,
  getDynamicProviderStatuses,
  addDynamicProvider,
  removeDynamicProvider,
  createProvider,
  deleteProvider,
  testLLMConfig,
} from '../lib/api/providers';
import type { NewProviderPayload } from '@/lib/api/providers';

export const providersKeys = {
  all: ['providers'] as const,
  llmConfig: ['providers', 'llm-config'] as const,
  dynamicProviders: ['dynamic-providers'] as const,
  dynamicProviderStatuses: ['dynamic-provider-statuses'] as const,
};

export function useProviders() {
  return useQuery({
    queryKey: providersKeys.all,
    queryFn: getProviders,
  });
}

export function useDynamicProviders() {
  return useQuery({
    queryKey: providersKeys.dynamicProviders,
    queryFn: getDynamicProviders,
  });
}

export function useDynamicProviderStatuses() {
  return useQuery({
    queryKey: providersKeys.dynamicProviderStatuses,
    queryFn: getDynamicProviderStatuses,
    refetchInterval: 10000, // Refresh statuses every 10s
  });
}

export function useLLMConfig() {
  return useQuery({
    queryKey: providersKeys.llmConfig,
    queryFn: getLLMConfig,
  });
}

export function useUpdateProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, config }: { id: string; config: Parameters<typeof updateProvider>[1] }) =>
      updateProvider(id, config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useSetActiveProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (providerId: string) => setActiveProvider(providerId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.llmConfig });
    },
  });
}

export function useUpdateLLMConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updateLLMConfig,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.llmConfig });
    },
  });
}

export function useCreateProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: NewProviderPayload) => createProvider(payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useDeleteProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => deleteProvider(name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useAddDynamicProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: addDynamicProvider,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.dynamicProviders });
    },
  });
}

export function useRemoveDynamicProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: removeDynamicProvider,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.dynamicProviders });
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useTestLLMConfig() {
  return useMutation({
    mutationFn: testLLMConfig,
  });
}
