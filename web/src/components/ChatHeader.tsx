import React from 'react';
import { Brain, PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { SessionInfo } from '@/lib/types/chat';

interface ChatHeaderProps {
  sidebarOpen: boolean;
  setSidebarOpen: (open: boolean | ((v: boolean) => boolean)) => void;
  sessionId: string | null;
  sessions: SessionInfo[];
  showThinking: boolean;
  setShowThinking: (show: boolean | ((v: boolean) => boolean)) => void;
}

export const ChatHeader: React.FC<ChatHeaderProps> = ({
  sidebarOpen,
  setSidebarOpen,
  sessionId,
  sessions,
  showThinking,
  setShowThinking,
}) => {
  const sessionTitle = sessionId
    ? sessions.find((s) => s.id === sessionId)?.title ?? 'New Chat'
    : null;

  return (
    <div className="flex items-center justify-between px-4 py-2 border-b border-sera-border flex-shrink-0">
      <div className="flex items-center gap-2 flex-1 min-w-0">
        <button
          onClick={() => setSidebarOpen((v) => !v)}
          className="p-1.5 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-text transition-colors"
          title={sidebarOpen ? 'Close sidebar' : 'Open sidebar'}
          aria-label="Toggle sidebar"
          aria-expanded={sidebarOpen}
        >
          {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
        </button>
        {sessionTitle && (
          <span className="text-xs text-sera-text-muted font-mono truncate">
            {sessionTitle}
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
};
