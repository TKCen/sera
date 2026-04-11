import { useEffect, useRef, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { IntrospectionMessage } from '@/hooks/useIntrospection';

interface IntrospectionFeedProps {
  messages: IntrospectionMessage[];
}

function getThoughtBadgeVariant(
  subType?: string
): 'accent' | 'success' | 'warning' | 'default' | 'error' | 'info' {
  switch (subType) {
    case 'reasoning':
      return 'accent';
    case 'tool-call':
    case 'observe':
      return 'warning';
    case 'reflect':
    case 'plan':
      return 'default';
    default:
      return 'default';
  }
}

function getSystemBadgeVariant(
  severity?: string
): 'accent' | 'success' | 'warning' | 'default' | 'error' | 'info' {
  switch (severity) {
    case 'error':
      return 'error';
    case 'warning':
      return 'warning';
    case 'info':
      return 'info';
    default:
      return 'default';
  }
}

function formatTimestamp(timestamp: number): string {
  const date = new Date(timestamp);
  const now = new Date();
  const diffMs = now.getTime() - timestamp;
  const diffSecs = Math.floor(diffMs / 1000);
  const diffMins = Math.floor(diffSecs / 60);

  if (diffSecs < 60) {
    return `${diffSecs}s ago`;
  } else if (diffMins < 60) {
    return `${diffMins}m ago`;
  } else {
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }
}

function formatToolArgs(args?: Record<string, unknown>): string {
  if (!args || Object.keys(args).length === 0) return '{}';
  try {
    return JSON.stringify(args).substring(0, 120);
  } catch {
    return '{}';
  }
}

export function IntrospectionFeed({ messages }: IntrospectionFeedProps) {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [showNewMessages, setShowNewMessages] = useState(false);

  // Auto-scroll to bottom when messages arrive (if enabled)
  useEffect(() => {
    if (autoScroll && messagesEndRef.current?.scrollIntoView) {
      messagesEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages, autoScroll]);

  // Handle scroll to detect if user scrolled up
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleScroll = () => {
      const atBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 100;
      setAutoScroll(atBottom);
      setShowNewMessages(!atBottom && messages.length > 0);
    };

    container.addEventListener('scroll', handleScroll);
    return () => container.removeEventListener('scroll', handleScroll);
  }, [messages.length]);

  const getAvatarColor = (sourceId: string): string => {
    if (sourceId === 'system') return 'bg-sera-warning/20 text-sera-warning';

    // Deterministic color based on agent ID
    const hash = sourceId.split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
    const colors = [
      'bg-sera-accent/20 text-sera-accent',
      'bg-sera-success/20 text-sera-success',
      'bg-sera-warning/20 text-sera-warning',
      'bg-sera-info/20 text-sera-info',
      'bg-blue-500/20 text-blue-400',
    ];
    return colors[hash % colors.length]!;
  };

  return (
    <div className="flex flex-col h-full">
      {/* Messages container */}
      <div ref={containerRef} className="flex-1 overflow-y-auto space-y-3 p-4 sera-card-static">
        {messages.length === 0 ? (
          <div className="flex items-center justify-center h-full text-sera-text-dim">
            <p>No activity yet. Agents will appear here as they work.</p>
          </div>
        ) : (
          <>
            {messages.map((msg) => (
              <div
                key={msg.id}
                className="group space-y-1 rounded-lg border border-sera-border bg-sera-surface-active p-3 hover:border-sera-border-active transition-colors"
              >
                {/* Header row: timestamp, avatar, name, badge */}
                <div className="flex items-center gap-2">
                  <span className="text-[10px] text-sera-text-dim whitespace-nowrap">
                    {formatTimestamp(msg.timestamp)}
                  </span>

                  {/* Avatar */}
                  <div
                    className={cn(
                      'w-6 h-6 rounded-full flex items-center justify-center text-xs font-semibold',
                      getAvatarColor(msg.sourceId)
                    )}
                  >
                    {msg.source[0]?.toUpperCase() ?? '?'}
                  </div>

                  {/* Source name */}
                  <span className="text-sm font-medium text-sera-text">{msg.source}</span>

                  {/* Type badge */}
                  {msg.type === 'thought' ? (
                    <Badge variant={getThoughtBadgeVariant(msg.subType)}>
                      {msg.subType ?? 'thought'}
                    </Badge>
                  ) : msg.type === 'system' ? (
                    <Badge variant={getSystemBadgeVariant(msg.metadata?.severity)}>
                      {msg.subType ?? 'system'}
                    </Badge>
                  ) : (
                    <Badge>{msg.type}</Badge>
                  )}
                </div>

                {/* Content */}
                <p className="text-sm text-sera-text break-words">{msg.content}</p>

                {/* Tool call details if present */}
                {msg.metadata?.toolName && (
                  <div className="mt-2 pt-2 border-t border-sera-border space-y-1">
                    <p className="text-xs text-sera-text-dim">
                      <span className="font-semibold">Tool:</span> {msg.metadata.toolName}
                    </p>
                    {msg.metadata.toolArgs && Object.keys(msg.metadata.toolArgs).length > 0 && (
                      <code className="text-[10px] text-sera-text-muted bg-sera-surface block p-2 rounded overflow-x-auto font-mono">
                        {formatToolArgs(msg.metadata.toolArgs)}
                      </code>
                    )}
                  </div>
                )}
              </div>
            ))}
            <div ref={messagesEndRef} className="h-0" />
          </>
        )}
      </div>

      {/* New messages button */}
      {showNewMessages && (
        <div className="px-4 py-2 border-t border-sera-border">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => {
              setAutoScroll(true);
              if (messagesEndRef.current?.scrollIntoView) {
                messagesEndRef.current.scrollIntoView({ behavior: 'smooth' });
              }
            }}
            className="w-full"
          >
            <ChevronDown size={14} />
            New messages
          </Button>
        </div>
      )}
    </div>
  );
}
