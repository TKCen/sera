import { useState, useRef, useEffect, useCallback } from 'react';
import { Send } from 'lucide-react';
import { cn } from '@/lib/utils';
import {
  chatStream,
  runOperatorTask,
  ApiError,
  type ChatStreamEvent,
  type OperatorTaskStatusCard,
} from '@/lib/api';
import { useAuth } from '@/contexts/auth-context';

interface Message {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  error?: string;
}

export function ChatView() {
  const { signOut } = useAuth();
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [thinking, setThinking] = useState(false);
  const [opTask, setOpTask] = useState('');
  const [opAgent, setOpAgent] = useState('sera');
  const [opHelper, setOpHelper] = useState('serahelper');
  const [opRunning, setOpRunning] = useState(false);
  const [opCard, setOpCard] = useState<OperatorTaskStatusCard | null>(null);
  const [opError, setOpError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const mountedRef = useRef(true);

  // Scroll to bottom whenever messages change
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, thinking]);

  // Abort stream on unmount
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      abortRef.current?.abort();
    };
  }, []);

  const handleEvent = useCallback((msgId: string, event: ChatStreamEvent) => {
    if (event.type === 'delta') {
      setMessages((prev) =>
        prev.map((m) =>
          m.id === msgId ? { ...m, content: m.content + event.delta } : m,
        ),
      );
    } else if (event.type === 'done') {
      setThinking(false);
    } else if (event.type === 'error') {
      // sera-aepj: gateway emits an `error` SSE frame when the runtime
      // returned an empty reply (or otherwise failed mid-turn). Surface
      // it on the assistant placeholder so the user sees the failure
      // instead of a stuck "thinking…" spinner.
      setMessages((prev) =>
        prev.map((m) => (m.id === msgId ? { ...m, error: event.error } : m)),
      );
      setThinking(false);
    }
  }, []);

  const sendMessage = useCallback(async () => {
    const text = input.trim();
    if (!text || thinking) return;

    const userMsg: Message = { id: crypto.randomUUID(), role: 'user', content: text };
    const assistantMsgId = crypto.randomUUID();
    const assistantMsg: Message = { id: assistantMsgId, role: 'assistant', content: '' };

    setMessages((prev) => [...prev, userMsg, assistantMsg]);
    setInput('');
    setThinking(true);

    const controller = new AbortController();
    abortRef.current = controller;

    try {
      await chatStream({
        message: text,
        signal: controller.signal,
        onEvent: (ev) => handleEvent(assistantMsgId, ev),
        onError: (err) => {
          if (err instanceof ApiError && err.status === 401) {
            setThinking(false);
            signOut();
            return;
          }
          const msg = err instanceof Error ? err.message : 'Stream error';
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantMsgId ? { ...m, error: msg } : m,
            ),
          );
          setThinking(false);
        },
      });
    } catch (err) {
      // AbortError is expected on unmount — ignore
      if (err instanceof DOMException && err.name === 'AbortError') return;
      if (err instanceof ApiError && err.status === 401) {
        if (mountedRef.current) setThinking(false);
        signOut();
        return;
      }
      const msg = err instanceof Error ? err.message : 'Unknown error';
      if (mountedRef.current) {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantMsgId ? { ...m, error: msg } : m,
          ),
        );
      }
    } finally {
      if (mountedRef.current) setThinking(false);
    }
  }, [input, thinking, handleEvent, signOut]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        void sendMessage();
      }
    },
    [sendMessage],
  );

  const runTask = useCallback(async () => {
    const task = opTask.trim();
    if (!task || opRunning) return;
    setOpRunning(true);
    setOpError(null);
    try {
      const card = await runOperatorTask({
        task,
        agent: opAgent.trim() || undefined,
        helper: opHelper.trim() || undefined,
      });
      if (!mountedRef.current) return;
      setOpCard(card);
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        signOut();
        return;
      }
      const msg = err instanceof Error ? err.message : 'Unknown error';
      if (mountedRef.current) setOpError(msg);
    } finally {
      if (mountedRef.current) setOpRunning(false);
    }
  }, [opTask, opAgent, opHelper, opRunning, signOut]);

  return (
    <div className="flex h-full flex-col">
      {/* Transcript */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {messages.length === 0 && (
          <div className="flex h-full items-center justify-center">
            <p className="text-sm text-zinc-500">Send a message to start chatting.</p>
          </div>
        )}
        {messages.map((msg) => (
          <div
            key={msg.id}
            className={cn(
              'flex',
              msg.role === 'user' ? 'justify-end' : 'justify-start',
            )}
          >
            <div
              className={cn(
                'max-w-[70%] rounded-lg px-3 py-2 text-sm',
                msg.role === 'user'
                  ? 'bg-zinc-700 text-zinc-100'
                  : 'bg-zinc-900 text-zinc-200 border border-zinc-800',
                msg.error && 'border-red-700 bg-red-950/30 text-red-300',
              )}
            >
              {msg.error ? (
                <span className="text-red-300">Error: {msg.error}</span>
              ) : msg.role === 'assistant' && msg.content === '' && thinking ? (
                <span className="text-zinc-500 italic">Thinking…</span>
              ) : (
                <span className="whitespace-pre-wrap">{msg.content}</span>
              )}
            </div>
          </div>
        ))}
        <div ref={bottomRef} />
      </div>

      {/* Operator task panel (sera-6zcb dogfood loop) */}
      <div
        className="border-t border-zinc-800 bg-zinc-950 p-3 space-y-2"
        data-testid="operator-task-panel"
      >
        <div className="flex items-center justify-between">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-zinc-400">
            Operator task
          </h2>
          {opCard?.status && (
            <span
              className={cn(
                'rounded px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide',
                opCard.blocked
                  ? 'bg-red-950/60 text-red-300 border border-red-800'
                  : 'bg-emerald-950/60 text-emerald-300 border border-emerald-800',
              )}
            >
              {opCard.status}
            </span>
          )}
        </div>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-[1fr_auto_auto_auto]">
          <input
            type="text"
            className="rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 focus:outline-none focus:ring-1 focus:ring-zinc-500"
            placeholder="Operator task… (e.g. summarize last release)"
            value={opTask}
            onChange={(e) => setOpTask(e.target.value)}
            disabled={opRunning}
            aria-label="Operator task"
          />
          <input
            type="text"
            className="rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 focus:outline-none focus:ring-1 focus:ring-zinc-500 sm:w-32"
            placeholder="agent"
            value={opAgent}
            onChange={(e) => setOpAgent(e.target.value)}
            disabled={opRunning}
            aria-label="Active agent"
          />
          <input
            type="text"
            className="rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 focus:outline-none focus:ring-1 focus:ring-zinc-500 sm:w-36"
            placeholder="helper"
            value={opHelper}
            onChange={(e) => setOpHelper(e.target.value)}
            disabled={opRunning}
            aria-label="Helper agent"
          />
          <button
            type="button"
            onClick={() => void runTask()}
            disabled={!opTask.trim() || opRunning}
            className={cn(
              'rounded-lg px-3 py-2 text-sm font-medium transition-colors',
              opTask.trim() && !opRunning
                ? 'bg-zinc-700 text-zinc-100 hover:bg-zinc-600'
                : 'bg-zinc-800 text-zinc-600 cursor-not-allowed',
            )}
          >
            {opRunning ? 'Running…' : 'Run'}
          </button>
        </div>
        {opError && (
          <div className="rounded border border-red-800 bg-red-950/40 px-2 py-1 text-xs text-red-300">
            Error: {opError}
          </div>
        )}
        {opCard && (
          <div
            className="rounded border border-zinc-800 bg-zinc-900/60 p-2 text-xs text-zinc-300 space-y-1"
            data-testid="operator-task-card"
          >
            <div className="grid grid-cols-2 gap-x-3 gap-y-0.5 sm:grid-cols-4">
              <Field label="Agent" value={opCard.active_agent} />
              <Field label="Helper" value={opCard.spawned_helper?.agent} />
              <Field label="Tool" value={opCard.handoff_tool} />
              <Field
                label="Helpers"
                value={
                  opCard.spawned_helper
                    ? `${opCard.spawned_helper.count ?? 0}/${opCard.spawned_helper.total ?? 0}`
                    : null
                }
              />
            </div>
            {opCard.latest_event && (
              <div>
                <span className="text-zinc-500">Latest event: </span>
                <span className="font-mono text-zinc-300">{opCard.latest_event}</span>
              </div>
            )}
            {opCard.result && (
              <div>
                <span className="text-zinc-500">Result: </span>
                <span className="whitespace-pre-wrap text-zinc-200">{opCard.result}</span>
              </div>
            )}
            <div className="flex flex-wrap gap-x-3 text-[11px] text-zinc-500">
              {opCard.session_key && <span>session={opCard.session_key}</span>}
              {opCard.audit_id && <span>audit={opCard.audit_id}</span>}
            </div>
          </div>
        )}
      </div>

      {/* Composer */}
      <div className="border-t border-zinc-800 bg-zinc-950 p-3" data-testid="chat-composer">
        <div className="flex items-end gap-2">
          <textarea
            className="flex-1 resize-none rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 focus:outline-none focus:ring-1 focus:ring-zinc-500"
            rows={2}
            placeholder="Message… (Enter to send, Shift+Enter for newline)"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            disabled={thinking}
          />
          <button
            type="button"
            onClick={() => void sendMessage()}
            disabled={!input.trim() || thinking}
            className={cn(
              'flex h-9 w-9 items-center justify-center rounded-lg transition-colors',
              input.trim() && !thinking
                ? 'bg-zinc-700 text-zinc-100 hover:bg-zinc-600'
                : 'bg-zinc-800 text-zinc-600 cursor-not-allowed',
            )}
            aria-label="Send"
          >
            <Send className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({ label, value }: { label: string; value: string | null | undefined }) {
  if (!value) return null;
  return (
    <div className="truncate">
      <span className="text-zinc-500">{label}: </span>
      <span className="text-zinc-200">{value}</span>
    </div>
  );
}
