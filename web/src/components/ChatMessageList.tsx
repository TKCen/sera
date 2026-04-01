import React, { useEffect, useRef } from 'react';
import type { Message } from '@/lib/types/chat';
import { ChatMessageBubble } from '@/components/ChatMessageBubble';

interface ChatMessageListProps {
  messages: Message[];
  showThinking: boolean;
  expandedThoughts: Set<string>;
  toggleThoughts: (msgId: string) => void;
  children?: React.ReactNode;
}

export const ChatMessageList: React.FC<ChatMessageListProps> = ({
  messages,
  showThinking,
  expandedThoughts,
  toggleThoughts,
  children,
}) => {
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // ── Auto-scroll ─────────────────────────────────────────────────────────────
  useEffect(() => {
    if (messagesEndRef.current?.scrollIntoView) {
      messagesEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages]);

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div
        className="flex-1 overflow-y-auto px-6 py-6 space-y-5"
        role="log"
        aria-live="polite"
      >
        {messages.map((msg) => (
          <ChatMessageBubble
            key={msg.id}
            msg={msg}
            showThinking={showThinking}
            isExpanded={expandedThoughts.has(msg.id)}
            onToggleThoughts={toggleThoughts}
          />
        ))}
        <div ref={messagesEndRef} />
      </div>
      {children && (
        <div className="px-6 py-4 border-t border-sera-border flex-shrink-0">
          <div className="max-w-3xl mx-auto">
            {children}
          </div>
        </div>
      )}
    </div>
  );
};
