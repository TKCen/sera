import { useState, useRef, useEffect, useCallback } from 'react';
import { Bot, Send, BrainCircuit, X } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { toast } from 'sonner';
import { useAgents, useAgentTasks, useCreateAgentTask } from '@/hooks/useAgents';
import { useChannel } from '@/hooks/useChannel';
import { ThoughtTimeline } from '@/components/ThoughtTimeline';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import type { ThoughtEvent } from '@/lib/api/types';

interface TokenPayload {
  token: string;
  done: boolean;
  messageId?: string;
}

interface ThoughtPayload extends ThoughtEvent {}

interface Message {
  id: string;
  role: 'user' | 'agent';
  content: string;
  streaming?: boolean;
  createdAt: string;
}

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
      <pre className={cn('bg-sera-bg border border-sera-border rounded-lg px-4 py-3 overflow-x-auto text-[0.8em] leading-relaxed', className)}>
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

export default function ChatPage() {
  const { data: agents, isLoading: agentsLoading } = useAgents();
  const [selectedAgent, setSelectedAgent] = useState<string>('');
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [streaming, setStreaming] = useState(false);
  const [thoughts, setThoughts] = useState<ThoughtEvent[]>([]);
  const [showThoughts, setShowThoughts] = useState(false);

  const createTask = useCreateAgentTask();
  const { data: history } = useAgentTasks(selectedAgent, 'chat');

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const streamingMsgId = useRef<string | null>(null);
  const historyInitializedForAgent = useRef('');

  const tokenPayload = useChannel<TokenPayload>(
    selectedAgent ? `tokens:${selectedAgent}` : '',
  );
  const thoughtPayload = useChannel<ThoughtPayload>(
    selectedAgent ? `thoughts:${selectedAgent}` : '',
  );

  // Load chat history once per agent selection — do not overwrite active session on refetch
  useEffect(() => {
    if (!history || historyInitializedForAgent.current === selectedAgent) return;
    historyInitializedForAgent.current = selectedAgent;
    const loaded: Message[] = [];
    for (const task of history) {
      if (task.input) {
        loaded.push({ id: `${task.id}-in`, role: 'user', content: task.input, createdAt: task.createdAt ?? '' });
      }
      if (task.output) {
        loaded.push({ id: `${task.id}-out`, role: 'agent', content: task.output, createdAt: task.completedAt ?? task.createdAt ?? '' });
      }
    }
    setMessages(loaded);
    setThoughts([]);
  }, [history, selectedAgent]);

  // Handle incoming token stream
  useEffect(() => {
    if (!tokenPayload || !streaming) return;
    const { token, done } = tokenPayload;

    setMessages((prev) => {
      const idx = prev.findIndex((m) => m.id === streamingMsgId.current);
      if (idx === -1) return prev;
      const updated = [...prev];
      updated[idx] = {
        ...updated[idx],
        content: updated[idx].content + token,
        streaming: !done,
      };
      return updated;
    });

    if (done) {
      setStreaming(false);
      streamingMsgId.current = null;
    }
  }, [tokenPayload, streaming]);

  // Handle incoming thoughts
  useEffect(() => {
    if (!thoughtPayload) return;
    setThoughts((prev) => [...prev, thoughtPayload]);
  }, [thoughtPayload]);

  useEffect(() => {
    if (messagesEndRef.current?.scrollIntoView) {
      messagesEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages]);

  // Select first agent automatically
  useEffect(() => {
    if (agents && agents.length > 0 && !selectedAgent) {
      setSelectedAgent(agents[0].metadata.name);
    }
  }, [agents, selectedAgent]);

  const handleAgentChange = useCallback((name: string) => {
    setSelectedAgent(name);
    setMessages([]);
    setThoughts([]);
    setStreaming(false);
    streamingMsgId.current = null;
    historyInitializedForAgent.current = '';
  }, []);

  async function handleSend() {
    const text = input.trim();
    if (!text || !selectedAgent || streaming) return;

    const userMsg: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: text,
      createdAt: new Date().toISOString(),
    };
    const agentMsgId = crypto.randomUUID();
    const agentMsg: Message = {
      id: agentMsgId,
      role: 'agent',
      content: '',
      streaming: true,
      createdAt: new Date().toISOString(),
    };

    setMessages((prev) => [...prev, userMsg, agentMsg]);
    setInput('');
    setStreaming(true);
    setThoughts([]);
    streamingMsgId.current = agentMsgId;

    try {
      await createTask.mutateAsync({ name: selectedAgent, input: text });
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to send message');
      setMessages((prev) => prev.filter((m) => m.id !== agentMsgId));
      setStreaming(false);
      streamingMsgId.current = null;
    }
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  }

  return (
    <div className="flex h-full">
      {/* Main chat column */}
      <div className="flex flex-col flex-1 min-w-0 h-full">
        {/* Top bar */}
        <div className="flex items-center gap-3 px-4 py-3 border-b border-sera-border flex-shrink-0">
          <div className="h-6 w-6 rounded bg-sera-accent-soft flex items-center justify-center">
            <Bot size={13} className="text-sera-accent" />
          </div>
          {agentsLoading ? (
            <Skeleton className="h-7 w-40" />
          ) : (
            <select
              value={selectedAgent}
              onChange={(e) => handleAgentChange(e.target.value)}
              className="sera-input h-8 py-0 text-sm w-auto max-w-xs"
            >
              {!agents?.length && <option value="">No agents</option>}
              {agents?.map((a) => (
                <option key={a.metadata.name} value={a.metadata.name}>
                  {a.metadata.displayName ?? a.metadata.name}
                </option>
              ))}
            </select>
          )}
          <div className="ml-auto flex items-center gap-2">
            <button
              onClick={() => setShowThoughts((v) => !v)}
              className={cn(
                'flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors',
                showThoughts
                  ? 'bg-sera-accent-soft text-sera-accent'
                  : 'text-sera-text-muted hover:bg-sera-surface-hover',
              )}
            >
              <BrainCircuit size={13} />
              Thoughts
              {thoughts.length > 0 && (
                <span className="ml-0.5 text-[10px] opacity-70">{thoughts.length}</span>
              )}
            </button>
          </div>
        </div>

        {/* Messages */}
        <div className="flex-1 overflow-y-auto px-4 py-4 space-y-4 min-h-0">
          {messages.length === 0 ? (
            <div className="flex items-center justify-center h-full">
              <p className="text-sm text-sera-text-muted">
                {selectedAgent ? 'Send a message to start the conversation.' : 'Select an agent above.'}
              </p>
            </div>
          ) : (
            messages.map((msg) => (
              <ChatMessage key={msg.id} message={msg} />
            ))
          )}
          <div ref={messagesEndRef} />
        </div>

        {/* Input */}
        <div className="px-4 py-3 border-t border-sera-border flex-shrink-0">
          <div className="flex items-end gap-2">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              disabled={streaming || !selectedAgent}
              placeholder={streaming ? 'Agent is responding…' : 'Message agent… (Enter to send, Shift+Enter for newline)'}
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
              onClick={() => { void handleSend(); }}
              disabled={streaming || !input.trim() || !selectedAgent}
              className="flex-shrink-0 h-[38px] w-[38px] rounded-lg bg-sera-accent text-sera-bg flex items-center justify-center disabled:opacity-40 disabled:cursor-not-allowed hover:brightness-110 transition-all"
            >
              <Send size={14} />
            </button>
          </div>
        </div>
      </div>

      {/* Thought timeline panel */}
      {showThoughts && (
        <div className="w-72 border-l border-sera-border flex flex-col flex-shrink-0 h-full">
          <div className="absolute top-3 right-3">
            <button
              onClick={() => setShowThoughts(false)}
              className="p-1 text-sera-text-muted hover:text-sera-text transition-colors"
            >
              <X size={12} />
            </button>
          </div>
          <ThoughtTimeline thoughts={thoughts} className="flex-1 min-h-0" />
        </div>
      )}
    </div>
  );
}

function ChatMessage({ message }: { message: Message }) {
  const isUser = message.role === 'user';

  return (
    <div className={cn('flex gap-3', isUser ? 'justify-end' : 'justify-start')}>
      {!isUser && (
        <div className="h-7 w-7 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0 mt-0.5">
          <Bot size={13} className="text-sera-accent" />
        </div>
      )}
      <div
        className={cn(
          'max-w-[75%] rounded-xl px-4 py-2.5 text-sm',
          isUser
            ? 'bg-sera-accent text-sera-bg'
            : 'bg-sera-surface border border-sera-border text-sera-text',
        )}
      >
        {isUser ? (
          <p className="whitespace-pre-wrap">{message.content}</p>
        ) : (
          <div className="chat-prose text-sm">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                code({ className, children, ...props }) {
                  const isBlock = /language-/.test(className ?? '');
                  return isBlock ? (
                    <CodeBlock className={className}>{children}</CodeBlock>
                  ) : (
                    <code className="text-sera-accent bg-sera-surface-active rounded px-1 py-0.5 font-mono text-[0.82em]" {...props}>
                      {children}
                    </code>
                  );
                },
              }}
            >
              {message.content}
            </ReactMarkdown>
            {message.streaming && (
              <span className="inline-block w-1.5 h-4 bg-sera-accent ml-0.5 animate-pulse" />
            )}
          </div>
        )}
      </div>
    </div>
  );
}
