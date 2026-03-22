import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as mcpApi from '@/lib/api/mcp';

export function useRegisterMCPServer() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (manifest: object) => mcpApi.registerMCPServer(manifest),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['tools'] });
    },
  });
}

export function useUnregisterMCPServer() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => mcpApi.unregisterMCPServer(name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['tools'] });
    },
  });
}
