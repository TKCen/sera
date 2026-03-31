import { Bot, User, Loader2 } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { cn } from '@/lib/utils';
import { ChatThoughtPanel } from '@/components/ChatThoughtPanel';
import { CodeBlock } from '@/components/CodeBlock';
import { MessageCopyButton } from '@/components/MessageCopyButton';
import type { Message } from '@/app/chat/page';

export interface ChatMessageBubbleProps {
  msg: Message;
  showThinking: boolean;
  isExpanded: boolean;
  onToggleThoughts: (id: string) => void;
}

export function ChatMessageBubble({
  msg,
  showThinking,
  isExpanded,
  onToggleThoughts,
}: ChatMessageBubbleProps) {
  return (
    <article
      className={cn('flex gap-3 group', msg.role === 'user' ? 'justify-end' : 'justify-start')}
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
          isExpanded={isExpanded}
          onToggleThoughts={onToggleThoughts}
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
            <div
              className="flex items-center gap-2"
              role="status"
              aria-label="Generating message..."
            >
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
          <span className="text-[10px] opacity-40">{msg.createdAt.toLocaleTimeString()}</span>
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
    </article>
  );
}
