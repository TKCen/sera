import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as agentsApi from '@/lib/api/agents';
import type { AgentManifest } from '@/lib/api/types';

export const agentsKeys = {
  all: ['agents'] as const,
  detail: (name: string) => ['agents', name] as const,
  tasks: (name: string, type?: string) => ['agents', name, 'tasks', type] as const,
  schedules: (name: string) => ['agents', name, 'schedules'] as const,
  memory: (name: string, scope?: string) => ['agents', name, 'memory', scope] as const,
  thoughts: (name: string, taskId?: string) => ['agents', name, 'thoughts', taskId] as const,
  logs: (name: string) => ['agents', name, 'logs'] as const,
};

export function useAgents() {
  return useQuery({
    queryKey: agentsKeys.all,
    queryFn: agentsApi.listAgents,
  });
}

export function useAgent(name: string) {
  return useQuery({
    queryKey: agentsKeys.detail(name),
    queryFn: () => agentsApi.getAgent(name),
    enabled: name.length > 0,
  });
}

export function useAgentManifestRaw(name: string) {
  return useQuery({
    queryKey: [...agentsKeys.detail(name), 'raw'],
    queryFn: () => agentsApi.getAgentManifestRaw(name),
    enabled: name.length > 0,
  });
}

export function useAgentLogs(name: string) {
  return useQuery({
    queryKey: agentsKeys.logs(name),
    queryFn: () => agentsApi.getAgentLogs(name),
    enabled: name.length > 0,
    refetchInterval: 3000,
  });
}

export function useAgentTasks(name: string, type?: string) {
  return useQuery({
    queryKey: agentsKeys.tasks(name, type),
    queryFn: () => agentsApi.getAgentTasks(name, type),
    enabled: name.length > 0,
  });
}

export function useAgentSchedules(name: string) {
  return useQuery({
    queryKey: agentsKeys.schedules(name),
    queryFn: () => agentsApi.getAgentSchedules(name),
    enabled: name.length > 0,
  });
}

export function useAgentMemory(name: string, scope?: string) {
  return useQuery({
    queryKey: agentsKeys.memory(name, scope),
    queryFn: () => agentsApi.getAgentMemory(name, scope),
    enabled: name.length > 0,
  });
}

export function useAgentThoughts(name: string, taskId?: string) {
  return useQuery({
    queryKey: agentsKeys.thoughts(name, taskId),
    queryFn: () => agentsApi.getAgentThoughts(name, taskId),
    enabled: name.length > 0,
  });
}

export function useCreateAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (manifest: AgentManifest) => agentsApi.createAgent(manifest),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useUpdateAgentManifest() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, manifest }: { name: string; manifest: AgentManifest }) =>
      agentsApi.updateAgentManifest(name, manifest),
    onSuccess: (_data, { name }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(name) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useStartAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => agentsApi.startAgent(name),
    onSuccess: (_data, name) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(name) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useStopAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => agentsApi.stopAgent(name),
    onSuccess: (_data, name) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(name) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useRestartAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => agentsApi.restartAgent(name),
    onSuccess: (_data, name) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.detail(name) });
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useCreateAgentTask() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, input }: { name: string; input: string }) =>
      agentsApi.createAgentTask(name, input),
    onSuccess: (_data, { name }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.tasks(name, 'chat') });
    },
  });
}

export function useReloadAgents() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: agentsApi.reloadAgents,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}

export function useDeleteAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => agentsApi.deleteAgent(name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: agentsKeys.all });
    },
  });
}
