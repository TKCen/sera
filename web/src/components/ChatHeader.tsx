import { Brain } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { SessionInfo } from '@/components/ChatSidebar';

interface ChatHeaderProps {
  sidebarToggle: React.ReactNode;
  sessionId: string | null;
  sessions: SessionInfo[];
  showThinking: boolean;
  setShowThinking: (value: boolean | ((prev: boolean) => boolean)) => void;
}

export function ChatHeader({
  sidebarToggle,
  sessionId,
  sessions,
  showThinking,
  setShowThinking,
}: ChatHeaderProps) {
  return (
    <div className="flex items-center justify-between px-4 py-2 border-b border-sera-border flex-shrink-0">
      <div className="flex items-center gap-2 flex-1 min-w-0">
        {sidebarToggle}
        {sessionId && (
          <span className="text-xs text-sera-text-muted font-mono truncate">
            {sessions.find((s) => s.id === sessionId)?.title ?? 'New Chat'}
          </span>
        )}
      </div>
      <button
        onClick={() => setShowThinking((v) => !v)}
        className={cn(
          'flex items-center gap-1.5 px-2 py-1 rounded text-[10px] font-medium transition-all border',
          showThinking
            ? 'bg-sera-accent/10 text-sera-accent border-sera-accent/20'
            : 'bg-sera-surface text-sera-text-muted border-sera-border hover:text-sera-text'
        )}
      >
        <Brain size={12} className={showThinking ? 'animate-pulse' : ''} />
        THINKING: {showThinking ? 'ON' : 'OFF'}
      </button>
    </div>
  );
}
