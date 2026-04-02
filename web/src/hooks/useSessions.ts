import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { request } from '@/lib/api/client';
import type { MessageThought } from '@/lib/api/types';

export interface SessionInfo {
  id: string;
  agentName: string;
  agentInstanceId?: string | null;
  title: string;
  messageCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface SessionMessage {
  id: string;
  sessionId: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  metadata?: { thoughts?: MessageThought[] };
  createdAt: string;
}

export interface SessionDetail extends SessionInfo {
  messages: SessionMessage[];
}

export const sessionsKeys = {
  all: (agentName?: string, agentInstanceId?: string) =>
    ['sessions', { agentName, agentInstanceId }] as const,
  detail: (id: string) => ['sessions', id] as const,
  commands: (agentId: string, sessionId: string) =>
    ['agents', agentId, 'sessions', sessionId, 'commands'] as const,
};

export function useSessions(agentName?: string, agentInstanceId?: string) {
  return useQuery({
    queryKey: sessionsKeys.all(agentName, agentInstanceId),
    queryFn: () => {
      const params = new URLSearchParams();
      if (agentInstanceId) params.append('agentInstanceId', agentInstanceId);
      else if (agentName) params.append('agent', agentName);
      return request<SessionInfo[]>(`/sessions?${params.toString()}`);
    },
  });
}

export function useSession(id: string) {
  return useQuery({
    queryKey: sessionsKeys.detail(id),
    queryFn: () => request<SessionDetail>(`/sessions/${id}`),
    enabled: !!id,
  });
}

export function useDeleteSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => request(`/sessions/${id}`, { method: 'DELETE' }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['sessions'] });
    },
  });
}

export function useRenameSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, title }: { id: string; title: string }) =>
      request(`/sessions/${id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ title }),
      }),
    onSuccess: (_data, { id }) => {
      void qc.invalidateQueries({ queryKey: sessionsKeys.detail(id) });
      void qc.invalidateQueries({ queryKey: ['sessions'] });
    },
  });
}

export function useCommandLogs(agentId: string, sessionId: string) {
  return useQuery({
    queryKey: sessionsKeys.commands(agentId, sessionId),
    queryFn: () =>
      request<any[]>(
        `/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/commands`
      ),
    enabled: !!agentId && !!sessionId,
  });
}
