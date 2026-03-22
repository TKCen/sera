import { useState, useRef, useEffect, useCallback } from 'react';
import {
  Send,
  Loader2,
  Bot,
  User,
  Brain,
  Eye,
  Map,
  Zap,
  RotateCcw,
  ChevronDown,
  Sparkles,
  Plus,
  MessageSquare,
  Trash2,
  PanelLeftClose,
  PanelLeftOpen,
  Wrench,
  CheckCircle2,
} from 'lucide-react';
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

// ── Thought step icons & colours ─────────────────────────────────────────────

const STEP_ICONS: Record<string, React.ReactNode> = {
  observe: <Eye size={11} />,
  plan: <Map size={11} />,
  act: <Zap size={11} />,
  reflect: <RotateCcw size={11} />,
  'tool-call': <Wrench size={11} />,
  'tool-result': <CheckCircle2 size={11} />,
  reasoning: <Brain size={11} />,
};

const STEP_COLORS: Record<string, string> = {
  observe: 'text-blue-400',
  plan: 'text-amber-400',
  act: 'text-emerald-400',
  reflect: 'text-purple-400',
  'tool-call': 'text-cyan-400',
  'tool-result': 'text-teal-400',
  reasoning: 'text-violet-400',
};

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

  // ── Render per-message thinking block ────────────────────────────────────────

  function renderThinkingBlock(msg: Message) {
    if (msg.role !== 'agent') return null;
    if (!showThinking) return null;
    if (msg.thoughts.length === 0 && !msg.streaming) return null;

    const isExpanded = expandedThoughts.has(msg.id);

    return (
      <div className="mb-2">
        <button
          onClick={() => toggleThoughts(msg.id)}
          className={cn(
            'flex items-center gap-1.5 text-[12px] font-medium transition-colors duration-200',
            msg.streaming && msg.thoughts.length > 0
              ? 'text-sera-accent'
              : 'text-sera-text-muted hover:text-sera-text'
          )}
        >
          <Sparkles
            size={13}
            className={
              msg.streaming && msg.thoughts.length > 0 ? 'animate-pulse text-sera-accent' : ''
            }
          />
          <span>{msg.streaming ? 'Thinking…' : 'Thought process'}</span>
          <ChevronDown
            size={12}
            className={cn('transition-transform duration-200', isExpanded && 'rotate-180')}
          />
        </button>

        <div
          className={cn(
            'overflow-hidden transition-all duration-300',
            isExpanded ? 'max-h-[1200px] opacity-100 mt-2' : 'max-h-0 opacity-0'
          )}
        >
          <div
            className={cn(
              'pl-3 border-l-2 py-1 space-y-2.5 transition-colors duration-300',
              msg.streaming ? 'border-sera-accent/50' : 'border-sera-border'
            )}
          >
            {msg.thoughts.map((thought, i) => {
              // ── Reasoning block ──────────────────────────────────────────────
              if (thought.stepType === 'reasoning') {
                const isLast = i === msg.thoughts.length - 1;
                return (
                  <details
                    key={`${thought.timestamp}-${i}`}
                    className="group animate-in fade-in duration-300"
                    open
                  >
                    <summary className="flex items-center gap-1.5 cursor-pointer list-none select-none mb-2">
                      <span
                        className={cn(
                          'text-violet-400 flex-shrink-0',
                          msg.streaming && isLast && 'animate-pulse'
                        )}
                      >
                        <Brain size={11} />
                      </span>
                      <span className="text-[11px] font-semibold text-violet-300">
                        {msg.streaming && isLast ? 'Reasoning…' : 'Reasoning'}
                      </span>
                      <ChevronDown
                        size={10}
                        className="ml-auto text-violet-400/60 transition-transform group-open:rotate-180"
                      />
                    </summary>
                    <div className="relative ml-3">
                      <div className="pl-3 border-l border-violet-400/25 text-[11.5px] text-sera-text-muted leading-relaxed whitespace-pre-wrap max-h-80 overflow-y-auto [scrollbar-width:thin]">
                        {thought.content}
                      </div>
                      <div className="absolute bottom-0 left-3 right-0 h-6 bg-gradient-to-t from-sera-surface to-transparent pointer-events-none" />
                    </div>
                  </details>
                );
              }

              // ── Tool-call block ──────────────────────────────────────────────
              if (thought.stepType === 'tool-call') {
                const lines = thought.content.split('\n');
                const toolName = (lines[0] ?? '').replace(/^Tool:\s*/, '');
                const rawParams = lines
                  .slice(1)
                  .join('\n')
                  .replace(/^Parameters:\s*/, '');
                let paramDisplay = rawParams;
                try {
                  paramDisplay = JSON.stringify(JSON.parse(rawParams), null, 2);
                } catch {
                  /* not JSON — keep as-is */
                }
                return (
                  <div
                    key={`${thought.timestamp}-${i}`}
                    className="animate-in fade-in slide-in-from-left-2 duration-200"
                  >
                    <div className="flex items-center gap-1.5 mb-1">
                      <span className={cn('flex-shrink-0', STEP_COLORS['tool-call'])}>
                        {STEP_ICONS['tool-call']}
                      </span>
                      <span className="text-[11px] font-semibold text-cyan-300">{toolName}</span>
                    </div>
                    {paramDisplay && (
                      <pre className="ml-4 text-[10.5px] text-sera-text-muted leading-relaxed bg-sera-bg/60 border border-sera-border rounded px-2 py-1.5 overflow-x-auto whitespace-pre-wrap break-all [scrollbar-width:thin]">
                        {paramDisplay}
                      </pre>
                    )}
                  </div>
                );
              }

              // ── Tool-result block ────────────────────────────────────────────
              if (thought.stepType === 'tool-result') {
                const raw = thought.content.startsWith('Result: ')
                  ? thought.content.substring(8)
                  : thought.content;

                type SearchResult = { title: string; url: string; text: string };
                let parsedResults: SearchResult[] | null = null;
                try {
                  const parsed: unknown = JSON.parse(raw);
                  if (
                    Array.isArray(parsed) &&
                    parsed.length > 0 &&
                    typeof parsed[0] === 'object' &&
                    parsed[0] !== null &&
                    'title' in parsed[0]
                  ) {
                    parsedResults = parsed as SearchResult[];
                  }
                } catch {
                  /* not JSON */
                }

                if (parsedResults) {
                  return (
                    <div
                      key={`${thought.timestamp}-${i}`}
                      className="animate-in fade-in slide-in-from-left-2 duration-200"
                    >
                      <div className="flex items-center gap-1.5 mb-1.5">
                        <span className={cn('flex-shrink-0', STEP_COLORS['tool-result'])}>
                          {STEP_ICONS['tool-result']}
                        </span>
                        <span className="text-[11px] font-semibold text-teal-300">
                          {parsedResults.length} result{parsedResults.length !== 1 ? 's' : ''}{' '}
                          fetched
                        </span>
                      </div>
                      <div className="ml-4 space-y-1.5">
                        {parsedResults.map((r, ri) => (
                          <div key={ri}>
                            <a
                              href={r.url}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="text-[11px] text-sera-accent hover:underline font-medium leading-tight block truncate"
                              title={r.url}
                            >
                              {r.title}
                            </a>
                            {r.text && r.text !== r.title && (
                              <p className="text-[10.5px] text-sera-text-muted leading-snug mt-0.5 line-clamp-2">
                                {r.text}
                              </p>
                            )}
                          </div>
                        ))}
                      </div>
                    </div>
                  );
                }

                return (
                  <div
                    key={`${thought.timestamp}-${i}`}
                    className="flex items-start gap-2 animate-in fade-in slide-in-from-left-2 duration-200"
                  >
                    <span className={cn('mt-0.5 flex-shrink-0', STEP_COLORS['tool-result'])}>
                      {STEP_ICONS['tool-result']}
                    </span>
                    <div className="text-[11px] leading-relaxed min-w-0">
                      <span className="font-semibold text-teal-300">Result: </span>
                      <span className="text-sera-text-muted break-all">
                        {raw.length > 300 ? raw.substring(0, 300) + '…' : raw}
                      </span>
                    </div>
                  </div>
                );
              }

              // ── Generic step ─────────────────────────────────────────────────
              return (
                <div
                  key={`${thought.timestamp}-${i}`}
                  className="flex items-start gap-2 animate-in fade-in slide-in-from-left-2 duration-200"
                >
                  <span
                    className={cn(
                      'mt-0.5 flex-shrink-0',
                      STEP_COLORS[thought.stepType] ?? 'text-sera-text-muted'
                    )}
                  >
                    {STEP_ICONS[thought.stepType] ?? <Brain size={11} />}
                  </span>
                  <span className="text-[11px] text-sera-text-muted leading-relaxed">
                    {thought.content}
                  </span>
                </div>
              );
            })}

            {msg.streaming && msg.thoughts.length === 0 && (
              <div className="flex items-center gap-2">
                <Loader2 size={11} className="animate-spin text-sera-accent" />
                <span className="text-[11px] text-sera-text-muted">
                  Waiting for agent thoughts…
                </span>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  // ── Session sidebar ───────────────────────────────────────────────────────────

  const groupedSessions = sessions.reduce<Record<string, SessionInfo[]>>((acc, s) => {
    const key = s.agentName || 'Unknown Agent';
    if (!acc[key]) acc[key] = [];
    acc[key]!.push(s);
    return acc;
  }, {});

  const sessionSidebar = (
    <div
      className={cn(
        'flex flex-col border-r border-sera-border bg-sera-bg transition-all duration-200 flex-shrink-0',
        sidebarOpen ? 'w-64 min-w-[256px]' : 'w-0 min-w-0 overflow-hidden'
      )}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-3 border-b border-sera-border">
        <span className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-muted">
          Sessions
        </span>
        <button
          onClick={startNewSession}
          className="p-1 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-accent transition-colors"
          title="New chat"
        >
          <Plus size={16} />
        </button>
      </div>

      {/* Agent selector */}
      <div className="px-3 py-2 border-b border-sera-border">
        {agentsLoading ? (
          <div className="h-6 bg-sera-surface rounded animate-pulse" />
        ) : agentsError ? (
          <div className="flex items-center gap-2">
            <span className="text-xs text-sera-error">Failed to load agents</span>
            <button
              onClick={() => void refetchAgents()}
              className="text-xs px-2 py-1 bg-sera-surface border border-sera-border rounded hover:bg-sera-surface-hover"
            >
              Retry
            </button>
          </div>
        ) : (
          <select
            value={selectedAgent}
            onChange={(e) => handleAgentChange(e.target.value)}
            className="w-full bg-sera-surface border border-sera-border rounded px-2 py-1 text-xs text-sera-text focus:outline-none focus:border-sera-accent"
          >
            {!agents?.length && <option value="">No agents</option>}
            {agents?.map((a) => (
              <option key={a.name} value={a.name}>
                {a.display_name ?? a.name}
              </option>
            ))}
          </select>
        )}
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto">
        {sessions.length === 0 ? (
          <div className="px-3 py-6 text-center">
            <MessageSquare size={20} className="text-sera-text-muted mx-auto mb-2" />
            <p className="text-[11px] text-sera-text-muted">No sessions yet</p>
          </div>
        ) : (
          <div className="py-2">
            {Object.entries(groupedSessions).map(([agentName, agentSessions]) => (
              <div key={agentName} className="mb-4">
                <div className="px-3 py-1 mb-1">
                  <span className="text-[10px] font-bold uppercase tracking-wider text-sera-text-muted flex items-center gap-1.5">
                    <Bot size={10} />
                    {agentName}
                  </span>
                </div>
                <div className="space-y-0.5">
                  {agentSessions.map((s) => (
                    <button
                      key={s.id}
                      onClick={() => void loadSession(s.id)}
                      className={cn(
                        'w-full text-left px-3 py-2 flex items-start gap-2 group transition-colors border-l-2',
                        sessionId === s.id
                          ? 'bg-sera-accent-soft border-sera-accent'
                          : 'hover:bg-sera-surface border-transparent'
                      )}
                    >
                      <MessageSquare
                        size={13}
                        className="text-sera-text-muted mt-0.5 flex-shrink-0"
                      />
                      <div className="flex-1 min-w-0">
                        <p className="text-xs text-sera-text truncate">{s.title}</p>
                        <p className="text-[10px] text-sera-text-muted mt-0.5">
                          {s.messageCount} messages · {new Date(s.updatedAt).toLocaleDateString()}
                        </p>
                      </div>
                      <button
                        onClick={(e) => void deleteSession(s.id, e)}
                        className="opacity-0 group-hover:opacity-100 p-0.5 rounded text-sera-text-muted hover:text-red-400 transition-all"
                        title="Delete session"
                      >
                        <Trash2 size={12} />
                      </button>
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );

  // ── Input bar ─────────────────────────────────────────────────────────────────

  const inputBar = (
    <div className="flex items-end gap-2">
      <textarea
        ref={inputRef}
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKeyDown}
        disabled={streaming || !selectedAgent}
        placeholder={
          streaming
            ? 'Agent is responding…'
            : selectedAgent
              ? 'Message agent… (Enter to send, Shift+Enter for newline)'
              : 'Select an agent above'
        }
        rows={1}
        className="sera-input flex-1 resize-none min-h-[38px] max-h-32 overflow-y-auto"
        style={{ height: 'auto' }}
        onInput={(e) => {
          const el = e.currentTarget;
          el.style.height = 'auto';
          el.style.height = `${Math.min(el.scrollHeight, 128)}px`;
        }}
      />
      <button
        onClick={() => void handleSend()}
        disabled={streaming || !input.trim() || !selectedAgent}
        className="flex-shrink-0 h-[38px] w-[38px] rounded-lg bg-sera-accent text-sera-bg flex items-center justify-center disabled:opacity-40 disabled:cursor-not-allowed hover:brightness-110 transition-all"
      >
        {streaming ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
      </button>
    </div>
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
        {sessionSidebar}
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
          <div className="w-full max-w-2xl">{inputBar}</div>
        </div>
      </div>
    );
  }

  // ── Conversation view ─────────────────────────────────────────────────────────

  return (
    <div className="flex h-full">
      {sessionSidebar}

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
                {renderThinkingBlock(msg)}

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
          <div className="max-w-3xl mx-auto">{inputBar}</div>
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
