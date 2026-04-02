import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as providersApi from '../lib/api/providers';
import type {
  NewProviderPayload,
  AddProviderPayload,
  DynamicProviderConfig,
  DynamicProviderStatus,
} from '@/lib/api/providers';

export type { DynamicProviderConfig, DynamicProviderStatus };

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
    queryFn: providersApi.getProviders,
  });
}

export function useDynamicProviders() {
  return useQuery({
    queryKey: providersKeys.dynamicProviders,
    queryFn: providersApi.getDynamicProviders,
  });
}

export function useDynamicProviderStatuses() {
  return useQuery({
    queryKey: providersKeys.dynamicProviderStatuses,
    queryFn: providersApi.getDynamicProviderStatuses,
    refetchInterval: 15000, // Refresh statuses every 15s
  });
}

export function useLLMConfig() {
  return useQuery({
    queryKey: providersKeys.llmConfig,
    queryFn: providersApi.getLLMConfig,
  });
}

export function useUpdateProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, config }: { id: string; config: any }) =>
      providersApi.updateProvider(id, config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useUpdateProviderConfig() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ modelName, config }: { modelName: string; config: any }) =>
      providersApi.updateProviderConfig(modelName, config),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useDefaultModel() {
  return useQuery({
    queryKey: ['default-model'],
    queryFn: providersApi.getDefaultModel,
  });
}

export function useSetDefaultModel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (modelName: string) => providersApi.setDefaultModel(modelName),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['default-model'] });
    },
  });
}

export function useSetActiveProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (providerId: string) => providersApi.setActiveProvider(providerId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.llmConfig });
    },
  });
}

export function useUpdateLLMConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: providersApi.updateLLMConfig,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.llmConfig });
    },
  });
}

export function useCreateProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: NewProviderPayload) => providersApi.createProvider(payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useDeleteProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => providersApi.deleteProvider(name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useAddDynamicProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: providersApi.addDynamicProvider,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.dynamicProviders });
    },
  });
}

export function useRemoveDynamicProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => providersApi.removeDynamicProvider(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.dynamicProviders });
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useTestLLMConfig() {
  return useMutation({
    mutationFn: providersApi.testLLMConfig,
  });
}

export function useProviderTemplates() {
  return useQuery({
    queryKey: providersKeys.templates,
    queryFn: providersApi.getProviderTemplates,
  });
}

export function useAddProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: AddProviderPayload) => providersApi.addProvider(payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: providersKeys.all });
    },
  });
}

export function useDiscoverModels() {
  return useMutation({
    mutationFn: (modelName: string) => providersApi.discoverModels(modelName),
  });
}

export function useTestDynamicConnection() {
  return useMutation({
    mutationFn: ({ baseUrl, apiKey }: { baseUrl: string; apiKey?: string }) =>
      providersApi.testDynamicConnection(baseUrl, apiKey),
  });
}

export function useTestProvider() {
  return useMutation({
    mutationFn: (modelName: string) => providersApi.testProvider(modelName),
  });
}
