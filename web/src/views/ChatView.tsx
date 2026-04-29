import { useState, useRef, useEffect, useCallback } from 'react';
import { Send } from 'lucide-react';
import { cn } from '@/lib/utils';
import { chatStream, ApiError, type ChatStreamEvent } from '@/lib/api';
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

      {/* Composer */}
      <div className="border-t border-zinc-800 bg-zinc-950 p-3">
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
