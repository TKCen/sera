import { useState, useRef, useEffect, useCallback } from 'react';
import { Bot, Brain, PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import type { PublicationContext } from 'centrifuge';
import { useAgents } from '@/hooks/useAgents';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';
import {
  useSessions,
  useDeleteSession,
  useRenameSession,
  useChatStream,
} from '@/hooks/useSessions';
import { cn } from '@/lib/utils';
import { toast } from '@/lib/toast';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { ChatSidebar } from '@/components/ChatSidebar';
import { ChatInputBar } from '@/components/ChatInputBar';
import { ChatMessageBubble } from '@/components/ChatMessageBubble';
import { EmptyState } from '@/components/EmptyState';

// ── Types ────────────────────────────────────────────────────────────────────

import type { Message, MessageThought } from '@/lib/api/types';
export type { Message, MessageThought };

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
}

// ── ChatPage ──────────────────────────────────────────────────────────────────

function ChatPageContent() {
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

  const { data: sessions = [], refetch: fetchSessions } = useSessions(
    selectedAgent,
    selectedAgentId
  ) as { data: SessionInfo[]; refetch: () => void };
  const { sendChatStream } = useChatStream();

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
        ...(event.toolName ? { toolName: event.toolName } : {}),
        ...(event.toolArgs ? { toolArgs: event.toolArgs } : {}),
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
        const { request } = await import('@/lib/api/client');
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
          .filter((m: SessionMessage) => m.role === 'user' || m.role === 'assistant')
          .map((m: SessionMessage) => ({
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

  const deleteSessionMutation = useDeleteSession();
  const renameSessionMutation = useRenameSession();

  const deleteSession = useCallback(
    async (id: string, e: React.MouseEvent) => {
      e.stopPropagation();
      try {
        await deleteSessionMutation.mutateAsync(id);
        if (sessionId === id) startNewSession();
      } catch (err) {
        const errMsg = err instanceof Error ? err.message : 'Failed to delete session';
        toast.error(errMsg);
      }
    },
    [sessionId, startNewSession, deleteSessionMutation]
  );

  const renameSession = useCallback(
    async (id: string, title: string) => {
      try {
        await renameSessionMutation.mutateAsync({ id, title });
      } catch (err) {
        const errMsg = err instanceof Error ? err.message : 'Failed to rename session';
        toast.error(errMsg);
      }
    },
    [renameSessionMutation]
  );

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

  // ── Selected agent status ────────────────────────────────────────────────────
  const selectedAgentData = agents?.find((a) => a.name === selectedAgent);
  const agentStatus = (selectedAgentData as Record<string, unknown> | undefined)?.status as
    | string
    | undefined;
  const isAgentUnavailable = agentStatus === 'error' || agentStatus === 'stopped';

  // ── Sidebar toggle button ─────────────────────────────────────────────────────

  const sidebarToggle = (
    <button
      onClick={() => setSidebarOpen((v) => !v)}
      className="p-1.5 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-text transition-colors"
      title={sidebarOpen ? 'Close sidebar' : 'Open sidebar'}
      aria-label="Toggle sidebar"
      aria-expanded={sidebarOpen}
    >
      {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
    </button>
  );

  // ── Empty state ───────────────────────────────────────────────────────────────

  if (messages.length === 0 && !streaming) {
    return (
      <main className="flex h-full">
        <ChatSidebar
          sessions={sessions}
          agents={agents}
          agentsLoading={agentsLoading}
          agentsError={agentsError}
          selectedAgent={selectedAgent}
          sessionId={sessionId}
          sidebarOpen={sidebarOpen}
          onAgentChange={handleAgentChange}
          onStartNewSession={startNewSession}
          onLoadSession={(id) => void loadSession(id)}
          onDeleteSession={(id, e) => void deleteSession(id, e)}
          onRenameSession={(id, title) => void renameSession(id, title)}
          onRefetchAgents={() => void refetchAgents()}
        />
        <div className="flex-1 flex flex-col items-center justify-center px-8 relative">
          <div className="absolute top-4 left-4">{sidebarToggle}</div>
          <div className="flex-1 w-full flex flex-col items-center justify-center">
            <EmptyState
              icon={<Bot size={32} className="text-sera-accent" />}
              title="How can I help you?"
              description={
                selectedAgent
                  ? `Chatting with ${selectedAgentData?.display_name ?? selectedAgent}`
                  : 'Select an agent from the sidebar to get started.'
              }
            />
            {isAgentUnavailable && (
              <div className="mb-4 px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm max-w-md text-center">
                Agent is {agentStatus} — messages may not be delivered. Try restarting from the{' '}
                <a href={`/agents/${selectedAgentId}`} className="underline hover:text-red-300">
                  agent detail page
                </a>
                .
              </div>
            )}
          </div>
          <div className="w-full max-w-2xl pb-8">
            <ChatInputBar
              inputRef={inputRef}
              input={input}
              setInput={setInput}
              handleKeyDown={handleKeyDown}
              streaming={streaming}
              selectedAgent={selectedAgent}
              handleSend={() => void handleSend()}
              onCancel={handleCancel}
              queueCount={queueCount}
            />
          </div>
        </div>
      </main>
    );
  }

  // ── Conversation view ─────────────────────────────────────────────────────────

  return (
    <main className="flex h-full">
      <ChatSidebar
        sessions={sessions}
        agents={agents}
        agentsLoading={agentsLoading}
        agentsError={agentsError}
        selectedAgent={selectedAgent}
        sessionId={sessionId}
        sidebarOpen={sidebarOpen}
        onAgentChange={handleAgentChange}
        onStartNewSession={startNewSession}
        onLoadSession={(id) => void loadSession(id)}
        onDeleteSession={(id, e) => void deleteSession(id, e)}
        onRenameSession={(id, title) => void renameSession(id, title)}
        onRefetchAgents={() => void refetchAgents()}
      />

      <div className="flex-1 flex flex-col min-w-0 h-full">
        {/* Top bar */}
        <div className="flex items-center justify-between px-4 py-2 border-b border-sera-border flex-shrink-0">
          <div className="flex items-center gap-2 flex-1 min-w-0">
            {sidebarToggle}
            {sessionId && (
              <span className="text-xs text-sera-text-muted font-mono truncate">
                {sessions.find((s) => s.id === sessionId)?.title ?? 'New Chat'}
              </span>
            )}
          </div>
          <button
            onClick={() => setShowThinking((v) => !v)}
            className={cn(
              'flex items-center gap-1.5 px-2 py-1 rounded text-[10px] font-medium transition-all border',
              showThinking
                ? 'bg-sera-accent/10 text-sera-accent border-sera-accent/20'
                : 'bg-sera-surface text-sera-text-muted border-sera-border hover:text-sera-text'
            )}
          >
            <Brain size={12} className={showThinking ? 'animate-pulse' : ''} />
            THINKING: {showThinking ? 'ON' : 'OFF'}
          </button>
        </div>

        {/* Messages */}
        <div
          className="flex-1 overflow-y-auto px-6 py-6 space-y-5 min-h-0"
          role="log"
          aria-live="polite"
        >
          {messages.map((msg) => (
            <ChatMessageBubble
              key={msg.id}
              msg={msg}
              showThinking={showThinking}
              isExpanded={expandedThoughts.has(msg.id)}
              onToggleThoughts={toggleThoughts}
            />
          ))}
          <div ref={messagesEndRef} />
        </div>

        {/* Input */}
        <div className="px-6 py-4 border-t border-sera-border flex-shrink-0">
          <div className="max-w-3xl mx-auto">
            <ChatInputBar
              inputRef={inputRef}
              input={input}
              setInput={setInput}
              handleKeyDown={handleKeyDown}
              streaming={streaming}
              selectedAgent={selectedAgent}
              handleSend={() => void handleSend()}
              onCancel={handleCancel}
              queueCount={queueCount}
            />
          </div>
        </div>
      </div>
    </main>
  );
}

export default function ChatPage() {
  return (
    <ErrorBoundary fallbackMessage="The chat interface encountered an error.">
      <ChatPageContent />
    </ErrorBoundary>
  );
}
