'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import { Send, Loader2, Bot, User, Brain, Eye, Map, Zap, RotateCcw, ChevronDown, Sparkles, Plus, MessageSquare, Trash2, PanelLeftClose, PanelLeftOpen, Wrench, CheckCircle2 } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { subscribeToThoughts, subscribeToStream, type ThoughtEvent } from '../../lib/centrifugo';

interface AgentInfo {
  name: string;
  role: string;
  displayName: string;
}

interface SessionInfo {
  id: string;
  agentName: string;
  title: string;
  messageCount: number;
  createdAt: string;
  updatedAt: string;
}

interface MessageThought {
  timestamp: string;
  stepType: string;
  content: string;
}

interface ChatMessage {
  id: string;
  sender: 'user' | 'sera';
  text: string;
  thoughts: MessageThought[];
  isStreaming: boolean;
  timestamp: Date;
}

const STEP_ICONS: Record<string, React.ReactNode> = {
  observe: <Eye size={11} />,
  plan: <Map size={11} />,
  act: <Zap size={11} />,
  reflect: <RotateCcw size={11} />,
  'tool-call': <Wrench size={11} />,
  'tool-result': <CheckCircle2 size={11} />,
};

const STEP_COLORS: Record<string, string> = {
  observe: 'text-blue-400',
  plan: 'text-amber-400',
  act: 'text-emerald-400',
  reflect: 'text-purple-400',
  'tool-call': 'text-cyan-400',
  'tool-result': 'text-teal-400',
};

export default function ChatPage() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [selectedAgentName, setSelectedAgentName] = useState<string>('general-assistant');
  const [expandedThoughts, setExpandedThoughts] = useState<Set<string>>(new Set());
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const currentStreamRef = useRef<(() => void) | null>(null);
  const currentThoughtsRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  // Fetch available agents
  useEffect(() => {
    const fetchAgents = async () => {
      try {
        const res = await fetch('/api/core/agents');
        if (res.ok) {
          const data = await res.json();
          setAgents(data);
          if (data.length > 0 && !data.find((a: AgentInfo) => a.name === selectedAgentName)) {
            setSelectedAgentName(data[0].name);
          }
        }
      } catch (err) {
        console.error('Failed to fetch agents:', err);
      }
    };
    fetchAgents();
  }, [selectedAgentName]);

  // Fetch sessions when agent changes
  useEffect(() => {
    fetchSessions();
  }, [selectedAgentName]);

  const fetchSessions = async () => {
    try {
      const res = await fetch(`/api/core/sessions?agent=${selectedAgentName}`);
      if (res.ok) {
        const data = await res.json();
        setSessions(data);
      }
    } catch (err) {
      console.error('Failed to fetch sessions:', err);
    }
  };

  const loadSession = async (id: string) => {
    try {
      const res = await fetch(`/api/core/sessions/${id}`);
      if (!res.ok) return;
      const data = await res.json();
      setSessionId(data.id);

      // Convert server messages to UI messages
      const uiMessages: ChatMessage[] = (data.messages || []).map((m: any, i: number) => ({
        id: m.id || `msg-${i}`,
        sender: m.role === 'user' ? 'user' : 'sera',
        text: m.content,
        thoughts: [],
        isStreaming: false,
        timestamp: new Date(m.createdAt || Date.now()),
      }));
      setMessages(uiMessages);
    } catch (err) {
      console.error('Failed to load session:', err);
    }
  };

  const startNewSession = () => {
    setSessionId(null);
    setMessages([]);
    inputRef.current?.focus();
  };

  const deleteSession = async (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await fetch(`/api/core/sessions/${id}`, { method: 'DELETE' });
      setSessions(prev => prev.filter(s => s.id !== id));
      if (sessionId === id) {
        startNewSession();
      }
    } catch (err) {
      console.error('Failed to delete session:', err);
    }
  };

  const toggleThoughts = useCallback((messageId: string) => {
    setExpandedThoughts(prev => {
      const next = new Set(prev);
      if (next.has(messageId)) {
        next.delete(messageId);
      } else {
        next.add(messageId);
      }
      return next;
    });
  }, []);

  const handleSend = useCallback(async () => {
    const trimmed = input.trim();
    if (!trimmed || isLoading) return;

    const userMsgId = crypto.randomUUID();
    const userMsg: ChatMessage = {
      id: userMsgId,
      sender: 'user',
      text: trimmed,
      thoughts: [],
      isStreaming: false,
      timestamp: new Date(),
    };

    // Create a placeholder for the streaming response
    const seraMsgId = crypto.randomUUID();
    const seraMsg: ChatMessage = {
      id: seraMsgId,
      sender: 'sera',
      text: '',
      thoughts: [],
      isStreaming: true,
      timestamp: new Date(),
    };

    setMessages(prev => [...prev, userMsg, seraMsg]);
    setInput('');
    setIsLoading(true);
    // Auto-expand thoughts while streaming
    setExpandedThoughts(prev => new Set(prev).add(seraMsgId));

    // Subscribe to thoughts for this agent
    const unsubThoughts = subscribeToThoughts(selectedAgentName, (event: ThoughtEvent) => {
      setMessages(prev => prev.map(msg =>
        msg.id === seraMsgId
          ? {
              ...msg,
              thoughts: [...msg.thoughts, {
                timestamp: event.timestamp,
                stepType: event.stepType,
                content: event.content,
              }],
            }
          : msg
      ));
    });
    currentThoughtsRef.current = unsubThoughts;

    try {
      // POST to the streaming endpoint — it returns immediately with the messageId
      const res = await fetch('/api/core/chat/stream', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          message: trimmed,
          sessionId,
          agentName: selectedAgentName,
        }),
      });
      const data = await res.json();

      if (!res.ok) throw new Error(data.error || 'Stream request failed');

      if (data.sessionId) setSessionId(data.sessionId);
      const messageId = data.messageId;

      // Subscribe to the streaming channel for token-by-token delivery
      const unsubStream = subscribeToStream(
        messageId,
        // onToken: accumulate text
        (token: string) => {
          setMessages(prev => prev.map(msg =>
            msg.id === seraMsgId
              ? { ...msg, text: msg.text + token }
              : msg
          ));
        },
        // onDone: mark streaming complete
        () => {
          setMessages(prev => prev.map(msg =>
            msg.id === seraMsgId
              ? { ...msg, isStreaming: false }
              : msg
          ));
          setIsLoading(false);
          // Auto-collapse thoughts after streaming
          setExpandedThoughts(prev => {
            const next = new Set(prev);
            next.delete(seraMsgId);
            return next;
          });
          // Clean up subscriptions
          unsubThoughts();
          currentThoughtsRef.current = null;
          inputRef.current?.focus();
          // Refresh sessions list
          fetchSessions();
        },
      );
      currentStreamRef.current = unsubStream;
    } catch (err: any) {
      setMessages(prev => prev.map(msg =>
        msg.id === seraMsgId
          ? {
              ...msg,
              text: `Error: ${err.message}. Check your LLM configuration in Settings.`,
              isStreaming: false,
            }
          : msg
      ));
      setIsLoading(false);
      unsubThoughts();
      currentThoughtsRef.current = null;
      inputRef.current?.focus();
    }
  }, [input, isLoading, sessionId, selectedAgentName]);

  // Cleanup subscriptions on unmount
  useEffect(() => {
    return () => {
      currentStreamRef.current?.();
      currentThoughtsRef.current?.();
    };
  }, []);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const agentSelector = (
    <div className="flex items-center gap-2">
      <label className="text-xs text-sera-text-muted">Agent:</label>
      <select
        value={selectedAgentName}
        onChange={(e) => {
          setSelectedAgentName(e.target.value);
          startNewSession();
        }}
        className="bg-sera-surface border border-sera-border rounded px-2 py-1 text-xs text-sera-text focus:outline-none focus:border-sera-accent"
      >
        {agents.map((agent) => (
          <option key={agent.name} value={agent.name}>
            {agent.displayName || agent.name}
          </option>
        ))}
      </select>
    </div>
  );

  // Session sidebar
  const sessionSidebar = (
    <div className={`
      flex flex-col border-r border-sera-border bg-sera-bg transition-all duration-200
      ${sidebarOpen ? 'w-64 min-w-[256px]' : 'w-0 min-w-0 overflow-hidden'}
    `}>
      {/* Sidebar header */}
      <div className="flex items-center justify-between px-3 py-3 border-b border-sera-border">
        <span className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim">Sessions</span>
        <button
          onClick={startNewSession}
          className="p-1 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-accent transition-colors"
          title="New chat"
        >
          <Plus size={16} />
        </button>
      </div>

      {/* Agent selector in sidebar */}
      <div className="px-3 py-2 border-b border-sera-border">
        {agentSelector}
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto">
        {sessions.length === 0 ? (
          <div className="px-3 py-6 text-center">
            <MessageSquare size={20} className="text-sera-text-dim mx-auto mb-2" />
            <p className="text-[11px] text-sera-text-dim">No sessions yet</p>
          </div>
        ) : (
          <div className="py-1">
            {sessions.map((s) => (
              <button
                key={s.id}
                onClick={() => loadSession(s.id)}
                className={`
                  w-full text-left px-3 py-2.5 flex items-start gap-2 group transition-colors
                  ${sessionId === s.id
                    ? 'bg-sera-accent-soft border-l-2 border-sera-accent'
                    : 'hover:bg-sera-surface border-l-2 border-transparent'
                  }
                `}
              >
                <MessageSquare size={14} className="text-sera-text-dim mt-0.5 flex-shrink-0" />
                <div className="flex-1 min-w-0">
                  <p className="text-xs text-sera-text truncate">{s.title}</p>
                  <p className="text-[10px] text-sera-text-dim mt-0.5">
                    {s.messageCount} messages · {new Date(s.updatedAt).toLocaleDateString()}
                  </p>
                </div>
                <button
                  onClick={(e) => deleteSession(s.id, e)}
                  className="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-sera-error/20 text-sera-text-dim hover:text-sera-error transition-all"
                  title="Delete session"
                >
                  <Trash2 size={12} />
                </button>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );

  // Renders the inline collapsible thinking section for a message
  const renderThinkingBlock = (msg: ChatMessage) => {
    if (msg.sender !== 'sera') return null;
    if (msg.thoughts.length === 0 && !msg.isStreaming) return null;

    const isExpanded = expandedThoughts.has(msg.id);

    return (
      <div className="mb-2">
        <button
          onClick={() => toggleThoughts(msg.id)}
          className={`
            flex items-center gap-1.5 text-[12px] font-medium transition-colors duration-200 group
            ${msg.isStreaming && msg.thoughts.length > 0
              ? 'text-sera-accent'
              : 'text-sera-text-dim hover:text-sera-text-muted'
            }
          `}
        >
          <Sparkles
            size={13}
            className={`${msg.isStreaming ? 'animate-pulse text-sera-accent' : ''}`}
          />
          <span>
            {msg.isStreaming && msg.thoughts.length === 0
              ? 'Thinking…'
              : msg.isStreaming
                ? 'Thinking…'
                : 'Thought process'
            }
          </span>
          <ChevronDown
            size={12}
            className={`transition-transform duration-200 ${isExpanded ? 'rotate-180' : ''}`}
          />
        </button>

        {/* Expandable thoughts container */}
        <div
          className={`
            overflow-hidden transition-all duration-300 ease-in-out
            ${isExpanded ? 'max-h-[500px] opacity-100 mt-2' : 'max-h-0 opacity-0'}
          `}
        >
          <div className="pl-3 border-l-2 border-sera-border space-y-1.5">
            {msg.thoughts.map((thought, i) => (
              <div
                key={`${thought.timestamp}-${i}`}
                className="flex items-start gap-2 animate-in fade-in slide-in-from-left-2 duration-200"
              >
                <span className={`mt-0.5 flex-shrink-0 ${STEP_COLORS[thought.stepType] || 'text-sera-text-dim'}`}>
                  {STEP_ICONS[thought.stepType] || <Brain size={11} />}
                </span>
                <span className="text-[11px] text-sera-text-muted leading-relaxed">
                  {thought.content}
                </span>
              </div>
            ))}
            {msg.isStreaming && msg.thoughts.length === 0 && (
              <div className="flex items-center gap-2">
                <Loader2 size={11} className="animate-spin text-sera-accent" />
                <span className="text-[11px] text-sera-text-dim">Waiting for agent thoughts…</span>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  };

  // Toggle sidebar button
  const sidebarToggle = (
    <button
      onClick={() => setSidebarOpen(prev => !prev)}
      className="p-1.5 rounded hover:bg-sera-surface text-sera-text-dim hover:text-sera-text transition-colors"
      title={sidebarOpen ? 'Close sidebar' : 'Open sidebar'}
    >
      {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
    </button>
  );

  // Input bar component (shared between empty state and conversation view)
  const inputBar = (
    <div className="sera-card-static p-1.5">
      <div className="flex items-end gap-2">
        <textarea
          ref={inputRef}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Message Sera…"
          rows={1}
          disabled={isLoading}
          className="flex-1 bg-transparent border-none py-2.5 px-3 text-sm text-sera-text
            placeholder:text-sera-text-dim resize-none
            focus:outline-none disabled:opacity-50"
        />
        <button
          onClick={handleSend}
          disabled={isLoading || !input.trim()}
          className="sera-btn-primary px-3 py-2.5 disabled:opacity-30 disabled:cursor-not-allowed"
        >
          {isLoading ? <Loader2 size={16} className="animate-spin" /> : <Send size={16} />}
        </button>
      </div>
    </div>
  );

  // Empty state (no messages)
  if (messages.length === 0 && !isLoading) {
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
            Start a conversation with Sera. Configure your LLM provider in Settings if you haven&apos;t already.
          </p>

          {/* Centered input */}
          <div className="w-full max-w-2xl">
            {inputBar}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      {sessionSidebar}
      <div className="flex-1 flex flex-col relative">
        {/* Top bar */}
        <div className="flex items-center gap-2 px-4 py-2 border-b border-sera-border">
          {sidebarToggle}
          {sessionId && (
            <span className="text-xs text-sera-text-dim font-mono truncate">
              {sessions.find(s => s.id === sessionId)?.title || 'New Chat'}
            </span>
          )}
        </div>

        {/* Messages */}
        <div className="flex-1 overflow-y-auto px-8 py-6 space-y-5">
          {messages.map((msg) => (
            <div key={msg.id} className={`flex gap-3 ${msg.sender === 'user' ? 'justify-end' : 'justify-start'}`}>
              {msg.sender === 'sera' && (
                <div className="w-8 h-8 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0 mt-0.5">
                  <Bot size={16} className="text-sera-accent" />
                </div>
              )}
              <div className={`max-w-[70%] rounded-xl px-4 py-3 ${
                msg.sender === 'user'
                  ? 'bg-sera-accent text-sera-bg'
                  : 'bg-sera-surface border border-sera-border text-sera-text'
              }`}>
                {/* Inline thinking block (Gemini-style) */}
                {renderThinkingBlock(msg)}

                {/* Message content */}
                <div className={`text-sm break-words leading-relaxed prose prose-sm max-w-none ${msg.sender === 'user' ? 'prose-invert text-sera-bg' : 'text-sera-text'}`}>
                  {msg.sender === 'user' ? (
                    <p className="whitespace-pre-wrap m-0">{msg.text}</p>
                  ) : msg.isStreaming && !msg.text ? (
                    <div className="flex items-center gap-2">
                      <Loader2 size={14} className="animate-spin text-sera-accent" />
                      <span className="text-xs text-sera-text-muted">Generating…</span>
                    </div>
                  ) : (
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.text}</ReactMarkdown>
                  )}
                  {/* Streaming cursor */}
                  {msg.isStreaming && msg.text && (
                    <span className="inline-block w-1.5 h-4 bg-sera-accent rounded-sm ml-0.5 animate-pulse align-text-bottom" />
                  )}
                </div>
                <span className="text-[10px] opacity-50 mt-1.5 block">
                  {msg.timestamp.toLocaleTimeString()}
                </span>
              </div>
              {msg.sender === 'user' && (
                <div className="w-8 h-8 rounded-lg bg-sera-surface border border-sera-border flex items-center justify-center flex-shrink-0 mt-0.5">
                  <User size={16} className="text-sera-text-muted" />
                </div>
              )}
            </div>
          ))}
          <div ref={messagesEndRef} />
        </div>

        {/* Input bar */}
        <div className="border-t border-sera-border p-4">
          <div className="max-w-3xl mx-auto">
            {inputBar}
          </div>
        </div>
      </div>
    </div>
  );
}
