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
  getProviderTemplates,
  addProvider,
  discoverModels,
} from '../lib/api/providers';
import type { NewProviderPayload, AddProviderPayload } from '@/lib/api/providers';

export const providersKeys = {
  all: ['providers'] as const,
  llmConfig: ['providers', 'llm-config'] as const,
  dynamicProviders: ['dynamic-providers'] as const,
  dynamicProviderStatuses: ['dynamic-provider-statuses'] as const,
  templates: ['provider-templates'] as const,
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
    refetchInterval: 15000, // Refresh statuses every 15s
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
    mutationFn: (config: Parameters<typeof updateLLMConfig>[0]) => updateLLMConfig(config),
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
    mutationFn: (config: Parameters<typeof addDynamicProvider>[0]) => addDynamicProvider(config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.dynamicProviders });
    },
  });
}

export function useRemoveDynamicProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => removeDynamicProvider(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.dynamicProviders });
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useTestLLMConfig() {
  return useMutation({
    mutationFn: () => testLLMConfig(),
  });
}

export function useProviderTemplates() {
  return useQuery({
    queryKey: providersKeys.templates,
    queryFn: getProviderTemplates,
  });
}

export function useAddProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: AddProviderPayload) => addProvider(payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useDiscoverModels() {
  return useMutation({
    mutationFn: (modelName: string) => discoverModels(modelName),
  });
}
