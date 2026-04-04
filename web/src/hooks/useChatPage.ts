import { useState, useRef, useEffect, useCallback } from 'react';
import type { PublicationContext } from 'centrifuge';
import { useAgents } from '@/hooks/useAgents';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';
import { sendChatStream } from '@/lib/api/chat';
import { request } from '@/lib/api/client';
import { toast } from '@/lib/toast';
import type { Message, MessageThought } from '@/lib/api/types';

interface SessionInfo {
  id: string;
  agentName: string;
  agentInstanceId?: string | null;
  title: string;
  messageCount: number;
  createdAt: string;
  updatedAt: string;
}

interface SessionDetail extends SessionInfo {
  messages: SessionMessage[];
}

interface SessionMessage {
  id: string;
  sessionId: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  metadata?: { thoughts?: MessageThought[] };
  createdAt: string;
}

interface TokenPayload {
  token: string;
  done: boolean;
  messageId?: string;
  error?: string;
}

interface ThoughtPayload {
  timestamp: string;
  stepType: string;
  content: string;
  agentId: string;
  agentDisplayName: string;
  toolName?: string;
  toolArgs?: Record<string, unknown>;
  toolCallId?: string;
}

export function useChatPage() {
  const {
    data: agents,
    isLoading: agentsLoading,
    isError: agentsError,
    refetch: refetchAgents,
  } = useAgents();
  const { client: centrifugoClient } = useCentrifugoContext();

  const [selectedAgent, setSelectedAgent] = useState<string>(
    () => sessionStorage.getItem('sera-chat-agent') ?? ''
  );
  const [selectedAgentId, setSelectedAgentId] = useState<string>(
    () => sessionStorage.getItem('sera-chat-agent-id') ?? ''
  );
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [streaming, setStreaming] = useState(false);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [showThinking, setShowThinking] = useState(true);
  const [expandedThoughts, setExpandedThoughts] = useState<Set<string>>(new Set());

  const inputRef = useRef<HTMLTextAreaElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const streamingMsgId = useRef<string | null>(null);
  const messageIdRef = useRef<string | null>(null);
  const messageQueue = useRef<string[]>([]);
  const [queueCount, setQueueCount] = useState(0);

  // ── Auto-scroll ─────────────────────────────────────────────────────────────
  useEffect(() => {
    if (messagesEndRef.current?.scrollIntoView) {
      messagesEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages]);

  // ── Auto-select agent (prefer running, persist to sessionStorage) ────────────
  useEffect(() => {
    if (agents && agents.length > 0 && !selectedAgent) {
      // Prefer a running agent over the first alphabetically
      const running = agents.find((a) => a.status === 'running');
      const pick = running ?? agents[0]!;
      setSelectedAgent(pick.name);
      setSelectedAgentId(pick.id);
    }
  }, [agents, selectedAgent]);

  // Persist selected agent to sessionStorage so it survives navigation
  useEffect(() => {
    if (selectedAgent) sessionStorage.setItem('sera-chat-agent', selectedAgent);
    if (selectedAgentId) sessionStorage.setItem('sera-chat-agent-id', selectedAgentId);
  }, [selectedAgent, selectedAgentId]);

  // ── Reset conversation when agent changes ────────────────────────────────────
  useEffect(() => {
    setMessages([]);
    setStreaming(false);
    setSessionId(null);
    streamingMsgId.current = null;
    messageIdRef.current = null;
    setExpandedThoughts(new Set());
  }, [selectedAgent]);

  // ── Fetch sessions whenever agent changes ────────────────────────────────────
  const fetchSessions = useCallback(async () => {
    try {
      // Filter by instance ID (unambiguous) — agent_name in the DB is inconsistent
      // (can be role name, instance name, or instance ID depending on how the session was created)
      const data = await request<SessionInfo[]>(
        selectedAgentId
          ? `/sessions?agentInstanceId=${encodeURIComponent(selectedAgentId)}`
          : selectedAgent
            ? `/sessions?agent=${encodeURIComponent(selectedAgent)}`
            : '/sessions'
      );
      setSessions(data);
    } catch {
      // Non-fatal — session list is best-effort
    }
  }, [selectedAgent, selectedAgentId]);

  useEffect(() => {
    void fetchSessions();
  }, [fetchSessions]);

  // ── Token stream — direct subscription ──────────────────────────────────────
  // Bypasses the useChannel→useState→useEffect chain which loses tokens when
  // React batches rapid state updates from the same WebSocket frame.
  useEffect(() => {
    const channelKey = selectedAgentId || selectedAgent;
    if (!centrifugoClient || !channelKey) return;
    const channel = `tokens:${channelKey}`;

    const existing = centrifugoClient.getSubscription(channel);
    if (existing) {
      existing.unsubscribe();
      existing.removeAllListeners();
      centrifugoClient.removeSubscription(existing);
    }

    const sub = centrifugoClient.newSubscription(channel);
    sub.on('publication', (ctx: PublicationContext) => {
      const { token, done, messageId, error } = ctx.data as TokenPayload;

      // Ignore tokens from a previous message's stream
      if (messageId != null && messageIdRef.current != null && messageId !== messageIdRef.current) {
        return;
      }
      if (!streamingMsgId.current) return;

      setMessages((prev) => {
        const idx = prev.findIndex((m) => m.id === streamingMsgId.current);
        if (idx === -1) return prev;
        const updated = [...prev];

        if (error) {
          // Surface LLM errors as a visible error message (#553)
          updated[idx] = {
            ...updated[idx]!,
            content: `**Error:** ${error}`,
            streaming: false,
          };
        } else {
          updated[idx] = {
            ...updated[idx]!,
            content: updated[idx]!.content + token,
            streaming: !done,
          };
        }

        return updated;
      });

      if (done) {
        // Delay clearing the streaming state to allow any remaining queued
        // tokens to be processed (Centrifugo may deliver them slightly after
        // the done packet). Without this, late tokens are silently dropped.
        setTimeout(() => {
          setStreaming(false);
          streamingMsgId.current = null;
          void fetchSessions();
          inputRef.current?.focus();
        }, 500);
      }
    });
    sub.subscribe();

    return () => {
      sub.unsubscribe();
      sub.removeAllListeners();
      centrifugoClient.removeSubscription(sub);
    };
  }, [centrifugoClient, selectedAgent, selectedAgentId, fetchSessions]);

  // ── Thought stream — direct subscription ────────────────────────────────────
  useEffect(() => {
    const channelKey = selectedAgentId || selectedAgent;
    if (!centrifugoClient || !channelKey) return;
    const channel = `thoughts:${channelKey}`;

    const existing = centrifugoClient.getSubscription(channel);
    if (existing) {
      existing.unsubscribe();
      existing.removeAllListeners();
      centrifugoClient.removeSubscription(existing);
    }

    const sub = centrifugoClient.newSubscription(channel);
    sub.on('publication', (ctx: PublicationContext) => {
      const event = ctx.data as ThoughtPayload;
      if (!streamingMsgId.current) return;

      const thought: MessageThought = {
        timestamp: event.timestamp,
        stepType: event.stepType,
        content: event.content,
        agentId: event.agentId,
        ...(event.toolName ? { toolName: event.toolName } : {}),
        ...(event.toolArgs ? { toolArgs: event.toolArgs } : {}),
        ...(event.toolCallId ? { toolCallId: event.toolCallId } : {}),
      };
      setMessages((prev) =>
        prev.map((msg) =>
          msg.id === streamingMsgId.current ? { ...msg, thoughts: [...msg.thoughts, thought] } : msg
        )
      );
    });
    sub.subscribe();

    return () => {
      sub.unsubscribe();
      sub.removeAllListeners();
      centrifugoClient.removeSubscription(sub);
    };
  }, [centrifugoClient, selectedAgent, selectedAgentId]);

  // ── Session actions ──────────────────────────────────────────────────────────

  const loadSession = useCallback(
    async (id: string) => {
      try {
        const data = await request<SessionDetail>(`/sessions/${id}`);
        setSessionId(data.id);
        if (data.agentName) setSelectedAgent(data.agentName);
        if (data.agentInstanceId) {
          setSelectedAgentId(data.agentInstanceId);
        } else if (data.agentName && agents) {
          // Fall back to looking up the instance ID from the agents list
          const agent = agents.find((a) => a.name === data.agentName);
          if (agent) setSelectedAgentId(agent.id);
        }

        const uiMessages: Message[] = (data.messages ?? [])
          .filter((m) => m.role === 'user' || m.role === 'assistant')
          .map((m) => ({
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
        // Non-fatal but should notify user
        const errMsg = err instanceof Error ? err.message : 'Failed to load session';
        toast.error(errMsg);
      }
    },
    [agents]
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
  }, []);

  const deleteSession = useCallback(
    async (id: string, e: React.MouseEvent) => {
      e.stopPropagation();
      try {
        await request(`/sessions/${id}`, { method: 'DELETE' });
        setSessions((prev) => prev.filter((s) => s.id !== id));
        if (sessionId === id) startNewSession();
      } catch (err) {
        // Non-fatal but should notify user
        const errMsg = err instanceof Error ? err.message : 'Failed to delete session';
        toast.error(errMsg);
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
      const errMsg = err instanceof Error ? err.message : 'Failed to rename session';
      toast.error(errMsg);
    }
  }, []);

  const toggleThoughts = useCallback((msgId: string) => {
    setExpandedThoughts((prev) => {
      const next = new Set(prev);
      if (next.has(msgId)) next.delete(msgId);
      else next.add(msgId);
      return next;
    });
  }, []);

  // ── Send message ─────────────────────────────────────────────────────────────

  const handleSend = useCallback(
    async (overrideText?: string) => {
      const text = (overrideText ?? input).trim();
      if (!text || !selectedAgent) return;

      // Queue the message if the agent is still streaming
      if (streaming) {
        messageQueue.current.push(text);
        setQueueCount(messageQueue.current.length);
        setInput('');
        return;
      }

      const userMsgId = crypto.randomUUID();
      const agentMsgId = crypto.randomUUID();

      const userMsg: Message = {
        id: userMsgId,
        role: 'user',
        content: text,
        thoughts: [],
        streaming: false,
        createdAt: new Date(),
      };
      const agentMsg: Message = {
        id: agentMsgId,
        role: 'agent',
        content: '',
        thoughts: [],
        streaming: true,
        createdAt: new Date(),
      };

      setMessages((prev) => [...prev, userMsg, agentMsg]);
      setInput('');
      setStreaming(true);
      setExpandedThoughts((prev) => new Set(prev).add(agentMsgId));
      streamingMsgId.current = agentMsgId;

      try {
        const { sessionId: newSessionId, messageId } = await sendChatStream(
          selectedAgent,
          text,
          sessionId ?? undefined,
          selectedAgentId || undefined
        );
        setSessionId(newSessionId);
        messageIdRef.current = messageId;
        // Refresh the session list so the sidebar shows the new/updated session
        void fetchSessions();
      } catch (err) {
        const errMsg = err instanceof Error ? err.message : 'Failed to send message';
        toast.error(errMsg);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === agentMsgId ? { ...m, content: `Error: ${errMsg}`, streaming: false } : m
          )
        );
        setStreaming(false);
        streamingMsgId.current = null;
        messageIdRef.current = null;
      }
    },
    [input, selectedAgent, selectedAgentId, streaming, sessionId, fetchSessions]
  );

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  };

  const handleAgentChange = useCallback(
    (name: string) => {
      setSelectedAgent(name);
      const agent = agents?.find((a) => a.name === name);
      setSelectedAgentId(agent?.id ?? '');
      // Reset session when switching agents
      setSessionId(null);
      setMessages([]);
    },
    [agents]
  );

  const handleCancel = useCallback(() => {
    if (!streaming) return;
    // Stop listening — mark the current streaming message as complete
    setMessages((prev) =>
      prev.map((m) =>
        m.id === streamingMsgId.current
          ? { ...m, content: m.content + '\n\n*(cancelled)*', streaming: false }
          : m
      )
    );
    setStreaming(false);
    streamingMsgId.current = null;
    messageIdRef.current = null;
  }, [streaming]);

  // ── Drain message queue when streaming finishes ──────────────────────────────
  const prevStreaming = useRef(false);
  useEffect(() => {
    // Fire when streaming transitions from true → false
    if (prevStreaming.current && !streaming && messageQueue.current.length > 0) {
      const next = messageQueue.current.shift()!;
      setQueueCount(messageQueue.current.length);
      void handleSend(next);
    }
    prevStreaming.current = streaming;
  }, [streaming, handleSend]);

  return {
    agents,
    agentsLoading,
    agentsError,
    refetchAgents,
    selectedAgent,
    selectedAgentId,
    messages,
    input,
    setInput,
    streaming,
    sessions,
    sessionId,
    sidebarOpen,
    setSidebarOpen,
    showThinking,
    setShowThinking,
    expandedThoughts,
    inputRef,
    messagesEndRef,
    loadSession,
    startNewSession,
    deleteSession,
    renameSession,
    toggleThoughts,
    handleSend,
    handleKeyDown,
    handleAgentChange,
    handleCancel,
    queueCount,
  };
}
