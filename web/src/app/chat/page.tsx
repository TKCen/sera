'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import { Send, Loader2, Bot, User, Brain, Eye, Map, Zap, RotateCcw, ChevronDown, ChevronUp } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { subscribeToThoughts, type ThoughtEvent, disconnectClient } from '../../lib/centrifugo';

interface AgentInfo {
  name: string;
  role: string;
  displayName: string;
}

interface ChatMessage {
  id: string;
  sender: 'user' | 'sera';
  text: string;
  thought?: string;
  timestamp: Date;
}

const STEP_ICONS: Record<string, React.ReactNode> = {
  observe: <Eye size={12} />,
  plan: <Map size={12} />,
  act: <Zap size={12} />,
  reflect: <RotateCcw size={12} />,
};

const STEP_COLORS: Record<string, string> = {
  observe: 'text-blue-400 bg-blue-400/10 border-blue-400/20',
  plan: 'text-amber-400 bg-amber-400/10 border-amber-400/20',
  act: 'text-emerald-400 bg-emerald-400/10 border-emerald-400/20',
  reflect: 'text-purple-400 bg-purple-400/10 border-purple-400/20',
};

export default function ChatPage() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [conversationId, setConversationId] = useState<string | null>(null);
  const [thoughts, setThoughts] = useState<ThoughtEvent[]>([]);
  const [showThoughts, setShowThoughts] = useState(true);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [selectedAgentName, setSelectedAgentName] = useState<string>('architect-prime');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const thoughtsEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    thoughtsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [thoughts]);

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

  // Subscribe to agent thoughts via Centrifugo
  useEffect(() => {
    if (!selectedAgentName) return;

    setThoughts([]); // Clear thoughts when switching agents
    const unsubscribe = subscribeToThoughts(selectedAgentName, (event) => {
      setThoughts(prev => [...prev.slice(-49), event]); // Keep last 50 thoughts
    });

    return () => {
      unsubscribe();
      disconnectClient();
    };
  }, [selectedAgentName]);

  const handleSend = useCallback(async () => {
    const trimmed = input.trim();
    if (!trimmed || isLoading) return;

    const userMsg: ChatMessage = {
      id: crypto.randomUUID(),
      sender: 'user',
      text: trimmed,
      timestamp: new Date(),
    };
    setMessages(prev => [...prev, userMsg]);
    setInput('');
    setIsLoading(true);
    // Clear thoughts for new message
    setThoughts([]);

    try {
      const res = await fetch('/api/core/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: trimmed, conversationId, agentName: selectedAgentName }),
      });
      const data = await res.json();

      if (!res.ok) throw new Error(data.error || 'Chat request failed');

      if (data.conversationId) setConversationId(data.conversationId);

      const seraMsg: ChatMessage = {
        id: crypto.randomUUID(),
        sender: 'sera',
        text: data.reply,
        thought: data.thought,
        timestamp: new Date(),
      };
      setMessages(prev => [...prev, seraMsg]);
    } catch (err: any) {
      const errorMsg: ChatMessage = {
        id: crypto.randomUUID(),
        sender: 'sera',
        text: `Error: ${err.message}. Check your LLM configuration in Settings.`,
        timestamp: new Date(),
      };
      setMessages(prev => [...prev, errorMsg]);
    } finally {
      setIsLoading(false);
      inputRef.current?.focus();
    }
  }, [input, isLoading, conversationId]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const agentSelector = (
    <div className="absolute top-4 right-8 z-10 flex items-center gap-2">
      <label className="text-xs text-sera-text-muted">Agent:</label>
      <select
        value={selectedAgentName}
        onChange={(e) => setSelectedAgentName(e.target.value)}
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

  // Empty state
  if (messages.length === 0 && !isLoading) {
    return (
      <div className="flex flex-col items-center justify-center h-full px-8 relative">
        {agentSelector}
        <div className="w-16 h-16 rounded-2xl bg-sera-accent-soft flex items-center justify-center mb-6">
          <Bot size={32} className="text-sera-accent" />
        </div>
        <h2 className="text-xl font-semibold text-sera-text mb-2">How can I help you?</h2>
        <p className="text-sm text-sera-text-muted mb-8 text-center max-w-md">
          Start a conversation with Sera. Configure your LLM provider in Settings if you haven&apos;t already.
        </p>

        {/* Centered input */}
        <div className="w-full max-w-2xl">
          <div className="sera-card-static p-1.5">
            <div className="flex items-end gap-2">
              <textarea
                ref={inputRef}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="Message Sera…"
                rows={1}
                className="flex-1 bg-transparent border-none py-2.5 px-3 text-sm text-sera-text
                  placeholder:text-sera-text-dim resize-none
                  focus:outline-none"
              />
              <button
                onClick={handleSend}
                disabled={!input.trim()}
                className="sera-btn-primary px-3 py-2.5 disabled:opacity-30 disabled:cursor-not-allowed"
              >
                <Send size={16} />
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full relative">
      {agentSelector}
      {/* Main content area */}
      <div className="flex flex-1 overflow-hidden">
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
                <div className={`text-sm break-words leading-relaxed prose prose-sm max-w-none ${msg.sender === 'user' ? 'prose-invert text-sera-bg' : 'text-sera-text'}`}>
                  {msg.sender === 'user' ? (
                    <p className="whitespace-pre-wrap m-0">{msg.text}</p>
                  ) : (
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.text}</ReactMarkdown>
                  )}
                </div>
                {msg.thought && msg.thought !== msg.text && (
                  <details className={`mt-2 border-t pt-2 ${msg.sender === 'user' ? 'border-sera-bg/20' : 'border-sera-border'}`}>
                    <summary className={`text-[11px] cursor-pointer transition-colors ${msg.sender === 'user' ? 'text-sera-bg/70 hover:text-sera-bg' : 'text-sera-text-dim hover:text-sera-accent'}`}>
                      Thought process
                    </summary>
                    <p className={`text-[11px] mt-1 italic ${msg.sender === 'user' ? 'text-sera-bg/80' : 'text-sera-text-muted'}`}>{msg.thought}</p>
                  </details>
                )}
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
          {isLoading && (
            <div className="flex gap-3 items-start">
              <div className="w-8 h-8 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
                <Bot size={16} className="text-sera-accent" />
              </div>
              <div className="bg-sera-surface border border-sera-border rounded-xl px-4 py-3 flex items-center gap-2">
                <Loader2 size={14} className="animate-spin text-sera-accent" />
                <span className="text-xs text-sera-text-muted">Thinking…</span>
              </div>
            </div>
          )}
          <div ref={messagesEndRef} />
        </div>

        {/* Thought Stream Panel */}
        {showThoughts && thoughts.length > 0 && (
          <div className="w-80 border-l border-sera-border bg-sera-surface/50 flex flex-col overflow-hidden">
            <div className="flex items-center justify-between px-4 py-3 border-b border-sera-border">
              <div className="flex items-center gap-2">
                <Brain size={14} className="text-sera-accent" />
                <span className="text-xs font-medium text-sera-text">Agent Thoughts</span>
              </div>
              <button
                onClick={() => setShowThoughts(false)}
                className="text-sera-text-dim hover:text-sera-text transition-colors"
              >
                <ChevronDown size={14} />
              </button>
            </div>
            <div className="flex-1 overflow-y-auto p-3 space-y-2">
              {thoughts.map((thought, i) => (
                <div
                  key={`${thought.timestamp}-${i}`}
                  className="animate-in fade-in slide-in-from-right-2 duration-300"
                >
                  <div className="flex items-start gap-2">
                    <span className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium border ${STEP_COLORS[thought.stepType] || 'text-sera-text-muted bg-sera-surface border-sera-border'}`}>
                      {STEP_ICONS[thought.stepType]}
                      {thought.stepType}
                    </span>
                    <span className="text-[10px] text-sera-text-dim mt-0.5">
                      {new Date(thought.timestamp).toLocaleTimeString()}
                    </span>
                  </div>
                  <p className="text-[11px] text-sera-text-muted mt-1 leading-relaxed pl-0.5">
                    {thought.content}
                  </p>
                </div>
              ))}
              <div ref={thoughtsEndRef} />
            </div>
          </div>
        )}
      </div>

      {/* Thought toggle + Input bar */}
      <div className="border-t border-sera-border p-4">
        <div className="max-w-3xl mx-auto">
          {/* Toggle thoughts button (when panel is hidden) */}
          {!showThoughts && thoughts.length > 0 && (
            <button
              onClick={() => setShowThoughts(true)}
              className="flex items-center gap-1.5 text-[11px] text-sera-text-dim hover:text-sera-accent transition-colors mb-2"
            >
              <Brain size={12} />
              <span>Show thought stream ({thoughts.length})</span>
              <ChevronUp size={12} />
            </button>
          )}
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
        </div>
      </div>
    </div>
  );
}
