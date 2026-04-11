import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as mcpApi from '@/lib/api/mcp';

export function useMCPServers() {
  return useQuery({
    queryKey: ['mcp-servers'],
    queryFn: () => mcpApi.listMCPServers(),
  });
}

export function useMCPServerHealth(name: string) {
  return useQuery({
    queryKey: ['mcp-servers', name, 'health'],
    queryFn: () => mcpApi.getMCPServerHealth(name),
    enabled: !!name,
  });
}

export function useRegisterMCPServer() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (manifest: object) => mcpApi.registerMCPServer(manifest),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['tools'] });
      void qc.invalidateQueries({ queryKey: ['mcp-servers'] });
    },
  });
}

export function useUnregisterMCPServer() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => mcpApi.unregisterMCPServer(name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['tools'] });
      void qc.invalidateQueries({ queryKey: ['mcp-servers'] });
    },
  });
}

export function useReloadMCPServer() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => mcpApi.reloadMCPServer(name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['tools'] });
      void qc.invalidateQueries({ queryKey: ['mcp-servers'] });
    },
  });
}
