import { renderHook, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  useCreateProvider,
  useUpdateLLMConfig,
  useDeleteProvider,
  providersKeys,
} from './useProviders';
import * as providersApi from '../lib/api/providers';
import React from 'react';

vi.mock('../lib/api/providers', () => ({
  createProvider: vi.fn(),
  updateLLMConfig: vi.fn(),
  deleteProvider: vi.fn(),
  // Add other required mocks if needed by the hooks
  getProviders: vi.fn(),
  updateProvider: vi.fn(),
  setActiveProvider: vi.fn(),
  getLLMConfig: vi.fn(),
  getDynamicProviders: vi.fn(),
  getDynamicProviderStatuses: vi.fn(),
  addDynamicProvider: vi.fn(),
  removeDynamicProvider: vi.fn(),
  testLLMConfig: vi.fn(),
  getProviderTemplates: vi.fn(),
  addProvider: vi.fn(),
  discoverModels: vi.fn(),
}));

const createTestQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
      mutations: {
        retry: false,
      },
    },
  });

describe('useProviders hooks', () => {
  let queryClient: QueryClient;
  let wrapper: React.FC<{ children: React.ReactNode }>;

  beforeEach(() => {
    queryClient = createTestQueryClient();
    wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    vi.clearAllMocks();
  });

  describe('useCreateProvider', () => {
    it('calls createProvider and invalidates providers query on success', async () => {
      const mockCreateProvider = vi.mocked(providersApi.createProvider);
      mockCreateProvider.mockResolvedValue({ success: true } as any);
      const invalidateQueriesSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useCreateProvider(), { wrapper });

      const payload = { name: 'test', type: 'cloud' as const, modelId: 'model-1' };
      result.current.mutate(payload);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));

      expect(mockCreateProvider).toHaveBeenCalledWith(payload);
      expect(invalidateQueriesSpy).toHaveBeenCalledWith({ queryKey: providersKeys.all });
    });
  });

  describe('useUpdateLLMConfig', () => {
    it('calls updateLLMConfig and invalidates llmConfig query on success', async () => {
      const mockUpdateLLMConfig = vi.mocked(providersApi.updateLLMConfig);
      mockUpdateLLMConfig.mockResolvedValue({ success: true } as any);
      const invalidateQueriesSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useUpdateLLMConfig(), { wrapper });

      const config = { defaultModel: 'gpt-4' };
      result.current.mutate(config);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));

      expect(mockUpdateLLMConfig).toHaveBeenCalledWith(config);
      expect(invalidateQueriesSpy).toHaveBeenCalledWith({ queryKey: providersKeys.llmConfig });
    });
  });

  describe('useDeleteProvider', () => {
    it('calls deleteProvider and invalidates providers query on success', async () => {
      const mockDeleteProvider = vi.mocked(providersApi.deleteProvider);
      mockDeleteProvider.mockResolvedValue({ success: true } as any);
      const invalidateQueriesSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useDeleteProvider(), { wrapper });

      const providerName = 'test-provider';
      result.current.mutate(providerName);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));

      expect(mockDeleteProvider).toHaveBeenCalledWith(providerName);
      expect(invalidateQueriesSpy).toHaveBeenCalledWith({ queryKey: providersKeys.all });
    });
  });
});
