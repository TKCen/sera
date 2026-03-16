import React from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export type MessageRole = 'user' | 'assistant' | 'system';

export interface MessageProps {
  id: string;
  role: MessageRole;
  content: string;
  isThinking?: boolean;
}

export const MessageBubble: React.FC<MessageProps> = ({ role, content, isThinking }) => {
  const isUser = role === 'user';
  const isSystem = role === 'system';

  return (
    <div
      className={cn(
        "flex w-full",
        isUser ? "justify-end" : "justify-start"
      )}
    >
      <div
        className={cn(
          "max-w-[85%] rounded-lg px-4 py-3 font-mono text-sm shadow-md",
          isUser
            ? "bg-primary/10 border border-primary/30 text-primary-foreground ml-auto glow-text"
            : isSystem
            ? "bg-muted/10 border border-muted/30 text-muted-foreground w-full text-center"
            : "bg-white/5 border border-white/10 text-foreground backdrop-blur-sm shadow-[0_0_15px_rgba(0,229,255,0.05)]",
          isThinking && "animate-pulse border-primary/50 shadow-[0_0_20px_rgba(0,229,255,0.2)]"
        )}
      >
        <div className="flex items-center gap-2 mb-1">
          <span
            className={cn(
              "text-[10px] uppercase tracking-widest font-bold",
              isUser ? "text-primary/70" : isSystem ? "text-muted-foreground/50" : "text-primary"
            )}
          >
            {isUser ? 'USER' : isSystem ? 'SYSTEM' : 'SERA_CORE'}
          </span>
          {!isUser && !isSystem && (
            <span className={cn("w-1.5 h-1.5 rounded-full", isThinking ? "bg-accent animate-pulse" : "bg-primary")} />
          )}
        </div>

        <div className={cn(
          "prose prose-invert max-w-none",
          "prose-p:leading-relaxed prose-pre:bg-black/50 prose-pre:border prose-pre:border-white/10",
          "prose-code:text-primary prose-code:bg-primary/10 prose-code:px-1 prose-code:py-0.5 prose-code:rounded",
          isUser ? "text-foreground" : "text-foreground/90"
        )}>
          {isThinking ? (
            <span className="flex items-center gap-1">
              Thinking<span className="animate-[bounce_1.4s_infinite] delay-0">.</span><span className="animate-[bounce_1.4s_infinite] delay-100">.</span><span className="animate-[bounce_1.4s_infinite] delay-200">.</span>
            </span>
          ) : (
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {content}
            </ReactMarkdown>
          )}
        </div>
      </div>
    </div>
  );
};

export default MessageBubble;
