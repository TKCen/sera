import { useState, useCallback } from 'react';
import { useQuery } from '@tanstack/react-query';
import { request } from '@/lib/api/client';
import { toast } from '@/lib/toast';

export interface SessionInfo {
  id: string;
  agentName: string;
  agentInstanceId?: string | null;
  title: string;
  messageCount: number;
  createdAt: string;
  updatedAt: string;
}

export function useSessionManagement(
  selectedAgent: string,
  selectedAgentId: string,
  sessionId: string | null,
  setSessionId: (id: string | null) => void,
  setMessages: (msgs: any[]) => void,
  setStreaming: (s: boolean) => void,
  streamingMsgId: React.MutableRefObject<string | null>,
  messageIdRef: React.MutableRefObject<string | null>,
  messageQueue: React.MutableRefObject<string[]>,
  setQueueCount: (c: number) => void,
  setExpandedThoughts: (s: Set<string>) => void,
  inputRef: React.RefObject<HTMLTextAreaElement | null>,
  agents: any[] | undefined
) {
  const [sessions, setSessions] = useState<SessionInfo[]>([]);

  const fetchSessions = useCallback(async () => {
    try {
      const data = await request<SessionInfo[]>(
        selectedAgentId
          ? `/sessions?agentInstanceId=${encodeURIComponent(selectedAgentId)}`
          : selectedAgent
            ? `/sessions?agent=${encodeURIComponent(selectedAgent)}`
            : '/sessions'
      );
      setSessions(data);
    } catch {
      // Non-fatal
    }
  }, [selectedAgent, selectedAgentId]);

  const loadSession = useCallback(
    async (id: string, setSelectedAgent: (n: string) => void, setSelectedAgentId: (id: string) => void) => {
      try {
        const data = await request<any>(`/sessions/${id}`);
        setSessionId(data.id);
        if (data.agentName) setSelectedAgent(data.agentName);
        if (data.agentInstanceId) {
          setSelectedAgentId(data.agentInstanceId);
        } else if (data.agentName && agents) {
          const agent = agents.find((a) => a.name === data.agentName);
          if (agent) setSelectedAgentId(agent.id);
        }
        const uiMessages = (data.messages ?? [])
          .filter((m: any) => m.role === 'user' || m.role === 'assistant')
          .map((m: any) => ({
            id: m.id,
            role: m.role === 'user' ? 'user' : 'agent',
            content: m.content,
            thoughts: Array.isArray(m.metadata?.thoughts) ? m.metadata!.thoughts! : [],
            streaming: false,
            createdAt: new Date(m.createdAt),
          }));
        setMessages(uiMessages);
        streamingMsgId.current = null;
        messageIdRef.current = null;
      } catch (err) {
        toast.error(err instanceof Error ? err.message : 'Failed to load session');
      }
    },
    [agents, setSessionId, setMessages, streamingMsgId, messageIdRef]
  );

  const startNewSession = useCallback(() => {
    setSessionId(null);
    setMessages([]);
    setStreaming(false);
    streamingMsgId.current = null;
    messageIdRef.current = null;
    messageQueue.current = [];
    setQueueCount(0);
    setExpandedThoughts(new Set());
    inputRef.current?.focus();
  }, [setSessionId, setMessages, setStreaming, streamingMsgId, messageIdRef, messageQueue, setQueueCount, setExpandedThoughts, inputRef]);

  const deleteSession = useCallback(
    async (id: string, e: React.MouseEvent) => {
      e.stopPropagation();
      try {
        await request(`/sessions/${id}`, { method: 'DELETE' });
        setSessions((prev) => prev.filter((s) => s.id !== id));
        if (sessionId === id) startNewSession();
      } catch (err) {
        toast.error(err instanceof Error ? err.message : 'Failed to delete session');
      }
    },
    [sessionId, startNewSession]
  );

  const renameSession = useCallback(async (id: string, title: string) => {
    try {
      await request(`/sessions/${id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ title }),
      });
      setSessions((prev) => prev.map((s) => (s.id === id ? { ...s, title } : s)));
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to rename session');
    }
  }, []);

  return {
    sessions,
    fetchSessions,
    loadSession,
    startNewSession,
    deleteSession,
    renameSession,
  };
}

export function useSessions(agentInstanceId?: string, agentName?: string) {
  return useQuery({
    queryKey: ['sessions', { agentInstanceId, agentName }],
    queryFn: () => {
      const params = new URLSearchParams();
      if (agentInstanceId) params.append('agentInstanceId', agentInstanceId);
      if (agentName) params.append('agent', agentName);
      return request<SessionInfo[]>(`/sessions?${params.toString()}`);
    },
  });
}
