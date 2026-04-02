import { renderHook, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import * as providersApi from '../lib/api/providers';
import {
  useProviders,
  useDynamicProviders,
  useDynamicProviderStatuses,
  useLLMConfig,
  useUpdateProvider,
  useSetActiveProvider,
  useUpdateLLMConfig,
  useCreateProvider,
  useDeleteProvider,
  useAddDynamicProvider,
  useRemoveDynamicProvider,
  useTestLLMConfig,
  useProviderTemplates,
  useAddProvider,
  useDiscoverModels,
} from './useProviders';

vi.mock('../lib/api/providers', () => ({
  getProviders: vi.fn(),
  updateProvider: vi.fn(),
  setActiveProvider: vi.fn(),
  getLLMConfig: vi.fn(),
  updateLLMConfig: vi.fn(),
  getDynamicProviders: vi.fn(),
  getDynamicProviderStatuses: vi.fn(),
  addDynamicProvider: vi.fn(),
  removeDynamicProvider: vi.fn(),
  createProvider: vi.fn(),
  deleteProvider: vi.fn(),
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

function TestWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = createTestQueryClient();
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

describe('useProviders hooks', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('Query hooks', () => {
    it('useProviders should fetch providers', async () => {
      const mockData = { providers: [], defaultModel: 'test' };
      vi.mocked(providersApi.getProviders).mockResolvedValue(mockData);

      const { result } = renderHook(() => useProviders(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockData);
      expect(providersApi.getProviders).toHaveBeenCalledTimes(1);
    });

    it('useDynamicProviders should fetch dynamic providers', async () => {
      const mockData = { dynamicProviders: [] };
      vi.mocked(providersApi.getDynamicProviders).mockResolvedValue(mockData);

      const { result } = renderHook(() => useDynamicProviders(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockData);
      expect(providersApi.getDynamicProviders).toHaveBeenCalledTimes(1);
    });

    it('useDynamicProviderStatuses should fetch dynamic provider statuses', async () => {
      const mockData = { statuses: [] };
      vi.mocked(providersApi.getDynamicProviderStatuses).mockResolvedValue(mockData);

      const { result } = renderHook(() => useDynamicProviderStatuses(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockData);
      expect(providersApi.getDynamicProviderStatuses).toHaveBeenCalledTimes(1);
    });

    it('useLLMConfig should fetch LLM config', async () => {
      const mockData: providersApi.LLMConfig = { model: 'test', baseUrl: 'url' };
      vi.mocked(providersApi.getLLMConfig).mockResolvedValue(mockData);

      const { result } = renderHook(() => useLLMConfig(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockData);
      expect(providersApi.getLLMConfig).toHaveBeenCalledTimes(1);
    });

    it('useProviderTemplates should fetch templates', async () => {
      const mockData = { templates: [] };
      vi.mocked(providersApi.getProviderTemplates).mockResolvedValue(mockData);

      const { result } = renderHook(() => useProviderTemplates(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockData);
      expect(providersApi.getProviderTemplates).toHaveBeenCalledTimes(1);
    });
  });

  describe('Mutation hooks', () => {
    it('useUpdateProvider should update provider and invalidate providers query', async () => {
      vi.mocked(providersApi.updateProvider).mockResolvedValue({ success: true });
      const { result } = renderHook(() => useUpdateProvider(), { wrapper: TestWrapper });

      await result.current.mutateAsync({ id: 'test-id', config: { model: 'new-model' } });

      expect(providersApi.updateProvider).toHaveBeenCalledWith('test-id', { model: 'new-model' });
    });

    it('useSetActiveProvider should set active provider and invalidate llmConfig query', async () => {
      vi.mocked(providersApi.setActiveProvider).mockResolvedValue({
        success: true,
        defaultModel: 'test',
      });
      const { result } = renderHook(() => useSetActiveProvider(), { wrapper: TestWrapper });

      await result.current.mutateAsync('test-model');

      expect(providersApi.setActiveProvider).toHaveBeenCalledWith('test-model');
    });

    it('useUpdateLLMConfig should update LLM config and invalidate llmConfig query', async () => {
      vi.mocked(providersApi.updateLLMConfig).mockResolvedValue({ success: true });
      const { result } = renderHook(() => useUpdateLLMConfig(), { wrapper: TestWrapper });

      const config: providersApi.LLMConfig = { model: 'test', baseUrl: 'url' };
      await result.current.mutateAsync(config);

      expect(providersApi.updateLLMConfig).toHaveBeenCalledWith(config);
    });

    it('useCreateProvider should create provider and invalidate providers query', async () => {
      vi.mocked(providersApi.createProvider).mockResolvedValue({ success: true });
      const { result } = renderHook(() => useCreateProvider(), { wrapper: TestWrapper });

      const payload = { name: 'test', type: 'local' as const, modelId: 'm1' };
      await result.current.mutateAsync(payload);

      expect(providersApi.createProvider).toHaveBeenCalledWith(payload);
    });

    it('useDeleteProvider should delete provider and invalidate providers query', async () => {
      vi.mocked(providersApi.deleteProvider).mockResolvedValue({ success: true });
      const { result } = renderHook(() => useDeleteProvider(), { wrapper: TestWrapper });

      await result.current.mutateAsync('test-name');

      expect(providersApi.deleteProvider).toHaveBeenCalledWith('test-name');
    });

    it('useAddDynamicProvider should add dynamic provider and invalidate dynamicProviders query', async () => {
      const mockResult: providersApi.DynamicProviderConfig = {
        id: '1',
        name: 'test',
        type: 'lm-studio',
        baseUrl: 'url',
        enabled: true,
        intervalMs: 1000,
      };
      vi.mocked(providersApi.addDynamicProvider).mockResolvedValue(mockResult);
      const { result } = renderHook(() => useAddDynamicProvider(), { wrapper: TestWrapper });

      const config: providersApi.DynamicProviderConfig = {
        id: '1',
        name: 'test',
        type: 'lm-studio',
        baseUrl: 'url',
        enabled: true,
        intervalMs: 1000,
      };
      await result.current.mutateAsync(config);

      expect(providersApi.addDynamicProvider).toHaveBeenCalledWith(config);
    });

    it('useRemoveDynamicProvider should remove dynamic provider and invalidate queries', async () => {
      vi.mocked(providersApi.removeDynamicProvider).mockResolvedValue(undefined);
      const { result } = renderHook(() => useRemoveDynamicProvider(), { wrapper: TestWrapper });

      await result.current.mutateAsync('test-id');

      expect(providersApi.removeDynamicProvider).toHaveBeenCalledWith('test-id');
    });

    it('useTestLLMConfig should test LLM config', async () => {
      vi.mocked(providersApi.testLLMConfig).mockResolvedValue({ success: true });
      const { result } = renderHook(() => useTestLLMConfig(), { wrapper: TestWrapper });

      await result.current.mutateAsync();

      expect(providersApi.testLLMConfig).toHaveBeenCalledTimes(1);
    });

    it('useAddProvider should add provider and invalidate providers query', async () => {
      vi.mocked(providersApi.addProvider).mockResolvedValue({
        modelName: 'test',
        result: { modelName: 'test', api: 'api' },
      });
      const { result } = renderHook(() => useAddProvider(), { wrapper: TestWrapper });

      const payload = { modelName: 'test', api: 'api' };
      await result.current.mutateAsync(payload);

      expect(providersApi.addProvider).toHaveBeenCalledWith(payload);
    });

    it('useDiscoverModels should discover models', async () => {
      vi.mocked(providersApi.discoverModels).mockResolvedValue({ provider: 'p', models: [] });
      const { result } = renderHook(() => useDiscoverModels(), { wrapper: TestWrapper });

      await result.current.mutateAsync('test-model');

      expect(providersApi.discoverModels).toHaveBeenCalledWith('test-model');
    });
  });
});
