import { useState } from 'react';
import { Bot, User, Copy, Check, Loader2 } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { cn } from '@/lib/utils';
import { ChatThoughtPanel } from '@/components/ChatThoughtPanel';
import type { Message } from './chat-types';

interface ChatMessageListProps {
  messages: Message[];
  showThinking: boolean;
  expandedThoughts: Set<string>;
  toggleThoughts: (msgId: string) => void;
  messagesEndRef: React.RefObject<HTMLDivElement | null>;
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

function MessageCopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        });
      }}
      className="p-1 rounded text-sera-text-dim hover:text-sera-text transition-colors"
      title="Copy message"
    >
      {copied ? <Check size={12} className="text-sera-success" /> : <Copy size={12} />}
    </button>
  );
}

export function ChatMessageList({
  messages,
  showThinking,
  expandedThoughts,
  toggleThoughts,
  messagesEndRef,
}: ChatMessageListProps) {
  return (
    <div className="flex-1 overflow-y-auto px-6 py-6 space-y-5 min-h-0">
      {messages.map((msg) => (
        <div
          key={msg.id}
          className={cn(
            'flex gap-3 group',
            msg.role === 'user' ? 'justify-end' : 'justify-start'
          )}
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

            <div className="flex items-center gap-1.5 mt-1.5">
              <span className="text-[10px] opacity-40">
                {msg.createdAt.toLocaleTimeString()}
              </span>
              {msg.role === 'agent' && msg.content && !msg.streaming && (
                <span className="opacity-0 group-hover:opacity-100 transition-opacity">
                  <MessageCopyButton text={msg.content} />
                </span>
              )}
            </div>
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
  );
}
