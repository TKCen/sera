import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as agentsApi from '@/lib/api/agents';
import { request } from '@/lib/api/client';
import type {
  CreateAgentInstanceParams,
  CreateGrantParams,
  PermissionDecisionParams,
} from '@/lib/api/types';

export const templateDiffKeys = {
  diff: (agentId: string) => ['template-diff', agentId] as const,
};

export const agentsKeys = {
  all: ['agents'] as const,
  detail: (id: string) => ['agents', id] as const,
  tasks: (id: string, type?: string) => ['agents', id, 'tasks', type] as const,
  schedules: (id: string) => ['agents', id, 'schedules'] as const,
  memory: (id: string, scope?: string) => ['agents', id, 'memory', scope] as const,
  thoughts: (id: string, taskId?: string) => ['agents', id, 'thoughts', taskId] as const,
  tools: (id: string) => ['agents', id, 'tools'] as const,
  logs: (id: string) => ['agents', id, 'logs'] as const,
  grants: (id: string) => ['agents', id, 'grants'] as const,
  delegations: (id: string) => ['agents', id, 'delegations'] as const,
  permissionRequests: (agentId?: string) => ['permission-requests', agentId] as const,
  pendingUpdates: () => ['agents', 'pending-updates'] as const,
};

export function useAgents() {
  return useQuery({
    queryKey: agentsKeys.all,
    queryFn: agentsApi.listAgents,
  });
}

export function useAgent(id: string) {
  return useQuery({
    queryKey: agentsKeys.detail(id),
    queryFn: () => agentsApi.getAgentInstance(id),
    enabled: id.length > 0,
  });
}

export function useRequest() {
  return { request };
}

export function useAgentTools(id: string) {
  return useQuery({
    queryKey: agentsKeys.tools(id),
    queryFn: () => agentsApi.getAgentTools(id),
    enabled: id.length > 0,
  });
}

export function useAgentDelegations(id: string) {
  return useQuery({
    queryKey: agentsKeys.delegations(id),
    queryFn: () => agentsApi.getAgentDelegations(id),
    enabled: id.length > 0,
  });
}

export function useAgentLogs(id: string) {
  return useQuery({
    queryKey: agentsKeys.logs(id),
    queryFn: () => agentsApi.getAgentLogs(id),
    enabled: id.length > 0,
    refetchInterval: 15000,
  });
}

export function useAgentTasks(id: string, status?: string) {
  return useQuery({
    queryKey: agentsKeys.tasks(id, status),
    queryFn: () => agentsApi.getAgentTasks(id, status),
    enabled: id.length > 0,
    refetchInterval: 10000,
  });
}

export function useCancelTask() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ agentId, taskId }: { agentId: string; taskId: string }) =>
      agentsApi.cancelAgentTask(agentId, taskId),
    onSuccess: (_data, { agentId }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.tasks(agentId) });
    },
  });
}

export function useClearStaleTasks() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ agentId, timeout }: { agentId: string; timeout?: number }) =>
      agentsApi.clearStaleTasks(agentId, timeout),
    onSuccess: (_data, { agentId }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.tasks(agentId) });
    },
  });
}

export function useAgentSchedules(id: string) {
  return useQuery({
    queryKey: agentsKeys.schedules(id),
    queryFn: () => agentsApi.getAgentSchedules(id),
    enabled: id.length > 0,
  });
}

export function useAgentMemory(id: string, scope?: string) {
  return useQuery({
    queryKey: agentsKeys.memory(id, scope),
    queryFn: () => agentsApi.getAgentMemory(id, scope),
    enabled: id.length > 0,
  });
}

export function useAgentThoughts(id: string, taskId?: string) {
  return useQuery({
    queryKey: agentsKeys.thoughts(id, taskId),
    queryFn: () => agentsApi.getAgentThoughts(id, taskId),
    enabled: id.length > 0,
  });
}

export function useCreateAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (params: CreateAgentInstanceParams) => agentsApi.createAgentInstance(params),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useStartAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => agentsApi.startAgent(id),
    onSuccess: (_data, id) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(id) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useStopAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => agentsApi.stopAgent(id),
    onSuccess: (_data, id) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(id) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useRestartAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string) => {
      await agentsApi.stopAgent(id);
      return agentsApi.startAgent(id);
    },
    onSuccess: (_data, id) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(id) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useCreateAgentTask() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: string }) =>
      agentsApi.createAgentTask(id, input),
    onSuccess: (_data, { id }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.tasks(id, 'chat') });
    },
  });
}

export function useUpdateAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      params,
    }: {
      id: string;
      params: Parameters<typeof agentsApi.updateAgentInstance>[1];
    }) => agentsApi.updateAgentInstance(id, params),
    onSuccess: (_data, { id }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(id) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useDeleteAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => agentsApi.deleteAgent(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

// ── Capability Grants ────────────────────────────────────────────────────────

export function useAgentGrants(id: string) {
  return useQuery({
    queryKey: agentsKeys.grants(id),
    queryFn: () => agentsApi.getAgentGrants(id),
    enabled: id.length > 0,
  });
}

export function usePendingUpdates() {
  return useQuery({
    queryKey: agentsKeys.pendingUpdates(),
    queryFn: agentsApi.getPendingUpdates,
    refetchInterval: 30000, // Poll every 30s
  });
}

export function useCreateGrant() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, params }: { id: string; params: CreateGrantParams }) =>
      agentsApi.createAgentGrant(id, params),
    onSuccess: (_data, { id }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.grants(id) });
    },
  });
}

export function useRevokeGrant() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, grantId }: { id: string; grantId: string }) =>
      agentsApi.revokeAgentGrant(id, grantId),
    onSuccess: (_data, { id }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.grants(id) });
    },
  });
}

// ── Permission Requests ─────────────────────────────────────────────────────

export function usePermissionRequests(agentId?: string) {
  return useQuery({
    queryKey: agentsKeys.permissionRequests(agentId),
    queryFn: () => agentsApi.listPermissionRequests(agentId),
    refetchInterval: 15000,
  });
}

export function useDecidePermission() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      requestId,
      params,
    }: {
      requestId: string;
      agentId: string;
      params: PermissionDecisionParams;
    }) => agentsApi.decidePermissionRequest(requestId, params),
    onSuccess: (_data, { agentId }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.permissionRequests(agentId) });
      void qc.invalidateQueries({ queryKey: agentsKeys.grants(agentId) });
    },
  });
}

export function useTemplateDiff(agentId: string) {
  return useQuery({
    queryKey: templateDiffKeys.diff(agentId),
    queryFn: () => agentsApi.getTemplateDiff(agentId),
    enabled: agentId.length > 0,
  });
}

export function useContextDebug(agentId: string, queryMessage: string) {
  return useQuery({
    queryKey: ['agent-context-debug', agentId, queryMessage],
    queryFn: () =>
      agentsApi.getAgentContextDebug(agentId, queryMessage),
    enabled: !!agentId && !!queryMessage,
  });
}

export function useApplyTemplateUpdate(agentId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (paths?: string[]) => agentsApi.applyTemplateUpdate(agentId, paths),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: templateDiffKeys.diff(agentId) });
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(agentId) });
    },
  });
}
