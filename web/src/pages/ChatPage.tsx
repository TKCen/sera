import { useState, useRef, useEffect, useCallback } from 'react';
import { Loader2, Bot, User, Brain, PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { PublicationContext } from 'centrifuge';
import { useAgents } from '@/hooks/useAgents';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';
import { sendChatStream } from '@/lib/api/chat';
import { request } from '@/lib/api/client';
import { cn } from '@/lib/utils';
import { toast } from '@/lib/toast';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { ChatSidebar } from '@/components/ChatSidebar';
import { ChatInputBar } from '@/components/ChatInputBar';
import { ChatThoughtPanel } from '@/components/ChatThoughtPanel';

// ── Types ────────────────────────────────────────────────────────────────────

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

interface MessageThought {
  timestamp: string;
  stepType: string;
  content: string;
}

interface TokenPayload {
  token: string;
  done: boolean;
  messageId?: string;
}

interface ThoughtPayload {
  timestamp: string;
  stepType: string;
  content: string;
  agentId: string;
  agentDisplayName: string;
}

interface Message {
  id: string;
  role: 'user' | 'agent';
  content: string;
  thoughts: MessageThought[];
  streaming: boolean;
  createdAt: Date;
}

// ── Code block with copy button ───────────────────────────────────────────────

function CodeBlock({ children, className }: { children?: React.ReactNode; className?: string }) {
  const [copied, setCopied] = useState(false);
  const code = String(children ?? '').trim();

  function handleCopy() {
    void navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }

  return (
    <div className="relative group my-2">
      <pre
        className={cn(
          'bg-sera-bg border border-sera-border rounded-lg px-4 py-3 overflow-x-auto text-[0.8em] leading-relaxed',
          className
        )}
      >
        <code>{children}</code>
      </pre>
      <button
        onClick={handleCopy}
        className="absolute top-2 right-2 px-2 py-0.5 rounded text-[10px] bg-sera-surface text-sera-text-muted opacity-0 group-hover:opacity-100 transition-opacity hover:text-sera-text"
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
    </div>
  );
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

  const [selectedAgent, setSelectedAgent] = useState<string>('');
  const [selectedAgentId, setSelectedAgentId] = useState<string>('');
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

  // ── Auto-scroll ─────────────────────────────────────────────────────────────
  useEffect(() => {
    if (messagesEndRef.current?.scrollIntoView) {
      messagesEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages]);

  // ── Auto-select first agent ──────────────────────────────────────────────────
  useEffect(() => {
    if (agents && agents.length > 0 && !selectedAgent) {
      setSelectedAgent(agents[0]!.name);
      setSelectedAgentId(agents[0]!.id);
    }
  }, [agents, selectedAgent]);

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
      const data = await request<SessionInfo[]>(
        selectedAgent ? `/sessions?agent=${encodeURIComponent(selectedAgent)}` : '/sessions'
      );
      setSessions(data);
    } catch {
      // Non-fatal — session list is best-effort
    }
  }, [selectedAgent]);

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
      const { token, done, messageId } = ctx.data as TokenPayload;

      // Ignore tokens from a previous message's stream
      if (messageId != null && messageIdRef.current != null && messageId !== messageIdRef.current) {
        return;
      }
      if (!streamingMsgId.current) return;

      setMessages((prev) => {
        const idx = prev.findIndex((m) => m.id === streamingMsgId.current);
        if (idx === -1) return prev;
        const updated = [...prev];
        updated[idx] = {
          ...updated[idx]!,
          content: updated[idx]!.content + token,
          streaming: !done,
        };
        return updated;
      });

      if (done) {
        setStreaming(false);
        streamingMsgId.current = null;
        void fetchSessions();
        inputRef.current?.focus();
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

  const loadSession = useCallback(async (id: string) => {
    try {
      const data = await request<SessionDetail>(`/sessions/${id}`);
      setSessionId(data.id);
      if (data.agentName) setSelectedAgent(data.agentName);

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
  }, []);

  const startNewSession = useCallback(() => {
    setSessionId(null);
    setMessages([]);
    setStreaming(false);
    streamingMsgId.current = null;
    messageIdRef.current = null;
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

  const toggleThoughts = useCallback((msgId: string) => {
    setExpandedThoughts((prev) => {
      const next = new Set(prev);
      if (next.has(msgId)) next.delete(msgId);
      else next.add(msgId);
      return next;
    });
  }, []);

  // ── Send message ─────────────────────────────────────────────────────────────

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text || !selectedAgent || streaming) return;

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
  }, [input, selectedAgent, selectedAgentId, streaming, sessionId]);

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
    },
    [agents]
  );

  // ── Sidebar toggle button ─────────────────────────────────────────────────────

  const sidebarToggle = (
    <button
      onClick={() => setSidebarOpen((v) => !v)}
      className="p-1.5 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-text transition-colors"
      title={sidebarOpen ? 'Close sidebar' : 'Open sidebar'}
    >
      {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
    </button>
  );

  // ── Empty state ───────────────────────────────────────────────────────────────

  if (messages.length === 0 && !streaming) {
    return (
      <div className="flex h-full">
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
          onRefetchAgents={() => void refetchAgents()}
        />
        <div className="flex-1 flex flex-col items-center justify-center px-8 relative">
          <div className="absolute top-4 left-4">{sidebarToggle}</div>
          <div className="w-16 h-16 rounded-2xl bg-sera-accent-soft flex items-center justify-center mb-6">
            <Bot size={32} className="text-sera-accent" />
          </div>
          <h2 className="text-xl font-semibold text-sera-text mb-2">How can I help you?</h2>
          <p className="text-sm text-sera-text-muted mb-8 text-center max-w-md">
            {selectedAgent
              ? `Chatting with ${agents?.find((a) => a.name === selectedAgent)?.display_name ?? selectedAgent}`
              : 'Select an agent from the sidebar to get started.'}
          </p>
          <div className="w-full max-w-2xl">
            <ChatInputBar
              inputRef={inputRef}
              input={input}
              setInput={setInput}
              handleKeyDown={handleKeyDown}
              streaming={streaming}
              selectedAgent={selectedAgent}
              handleSend={() => void handleSend()}
            />
          </div>
        </div>
      </div>
    );
  }

  // ── Conversation view ─────────────────────────────────────────────────────────

  return (
    <div className="flex h-full">
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
        <div className="flex-1 overflow-y-auto px-6 py-6 space-y-5 min-h-0">
          {messages.map((msg) => (
            <div
              key={msg.id}
              className={cn('flex gap-3', msg.role === 'user' ? 'justify-end' : 'justify-start')}
            >
              {msg.role === 'agent' && (
                <div className="w-8 h-8 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0 mt-0.5">
                  <Bot size={16} className="text-sera-accent" />
                </div>
              )}
              <div
                className={cn(
                  'max-w-[72%] rounded-xl px-4 py-3',
                  msg.role === 'user'
                    ? 'bg-sera-accent text-sera-bg'
                    : 'bg-sera-surface border border-sera-border text-sera-text'
                )}
              >
                {/* Inline thinking block */}
                <ChatThoughtPanel
                  msg={msg}
                  showThinking={showThinking}
                  isExpanded={expandedThoughts.has(msg.id)}
                  onToggleThoughts={toggleThoughts}
                />

                {/* Message content */}
                <div
                  className={cn(
                    'text-sm break-words leading-relaxed max-w-none',
                    msg.role === 'user' ? 'text-sera-bg' : 'chat-prose'
                  )}
                >
                  {msg.role === 'user' ? (
                    <p className="whitespace-pre-wrap m-0">{msg.content}</p>
                  ) : msg.streaming && !msg.content ? (
                    <div className="flex items-center gap-2">
                      <Loader2 size={14} className="animate-spin text-sera-accent" />
                      <span className="text-xs text-sera-text-muted">Generating…</span>
                    </div>
                  ) : (
                    <ReactMarkdown
                      remarkPlugins={[remarkGfm]}
                      components={{
                        code({ className, children, ...props }) {
                          const isBlock = /language-/.test(className ?? '');
                          return isBlock ? (
                            <CodeBlock className={className}>{children}</CodeBlock>
                          ) : (
                            <code
                              className="text-sera-accent bg-sera-surface-active rounded px-1 py-0.5 font-mono text-[0.82em]"
                              {...props}
                            >
                              {children}
                            </code>
                          );
                        },
                      }}
                    >
                      {msg.content}
                    </ReactMarkdown>
                  )}
                  {msg.streaming && msg.content && (
                    <span className="inline-block w-1.5 h-4 bg-sera-accent rounded-sm ml-0.5 animate-pulse align-text-bottom" />
                  )}
                </div>

                <span className="text-[10px] opacity-40 mt-1.5 block">
                  {msg.createdAt.toLocaleTimeString()}
                </span>
              </div>
              {msg.role === 'user' && (
                <div className="w-8 h-8 rounded-lg bg-sera-surface border border-sera-border flex items-center justify-center flex-shrink-0 mt-0.5">
                  <User size={16} className="text-sera-text-muted" />
                </div>
              )}
            </div>
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
            />
          </div>
        </div>
      </div>
    </div>
  );
}

export default function ChatPage() {
  return (
    <ErrorBoundary fallbackMessage="The chat interface encountered an error.">
      <ChatPageContent />
    </ErrorBoundary>
  );
}
