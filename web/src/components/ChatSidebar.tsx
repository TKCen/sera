import React from 'react';
import { Plus, Bot, MessageSquare, Trash2 } from 'lucide-react';
import { cn } from '@/lib/utils';

export interface SessionInfo {
  id: string;
  agentName: string;
  agentInstanceId?: string | null;
  title: string;
  messageCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface AgentInfo {
  id: string;
  name: string;
  display_name?: string | null;
}

interface ChatSidebarProps {
  sessions: SessionInfo[];
  agents: AgentInfo[] | undefined;
  agentsLoading: boolean;
  agentsError: boolean;
  selectedAgent: string;
  sessionId: string | null;
  sidebarOpen: boolean;
  onAgentChange: (name: string) => void;
  onStartNewSession: () => void;
  onLoadSession: (id: string) => void;
  onDeleteSession: (id: string, e: React.MouseEvent) => void;
  onRefetchAgents: () => void;
}

export function ChatSidebar({
  sessions,
  agents,
  agentsLoading,
  agentsError,
  selectedAgent,
  sessionId,
  sidebarOpen,
  onAgentChange,
  onStartNewSession,
  onLoadSession,
  onDeleteSession,
  onRefetchAgents,
}: ChatSidebarProps) {
  const groupedSessions = sessions.reduce<Record<string, SessionInfo[]>>((acc, s) => {
    const key = s.agentName || 'Unknown Agent';
    if (!acc[key]) acc[key] = [];
    acc[key]!.push(s);
    return acc;
  }, {});

  return (
    <div
      className={cn(
        'flex flex-col border-r border-sera-border bg-sera-bg transition-all duration-200 flex-shrink-0',
        sidebarOpen ? 'w-64 min-w-[256px]' : 'w-0 min-w-0 overflow-hidden'
      )}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-3 border-b border-sera-border">
        <span className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-muted">
          Sessions
        </span>
        <button
          onClick={onStartNewSession}
          className="p-1 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-accent transition-colors"
          title="New chat"
        >
          <Plus size={16} />
        </button>
      </div>

      {/* Agent selector */}
      <div className="px-3 py-2 border-b border-sera-border">
        {agentsLoading ? (
          <div className="h-6 bg-sera-surface rounded animate-pulse" />
        ) : agentsError ? (
          <div className="flex items-center gap-2">
            <span className="text-xs text-sera-error">Failed to load agents</span>
            <button
              onClick={onRefetchAgents}
              className="text-xs px-2 py-1 bg-sera-surface border border-sera-border rounded hover:bg-sera-surface-hover"
            >
              Retry
            </button>
          </div>
        ) : (
          <select
            value={selectedAgent}
            onChange={(e) => onAgentChange(e.target.value)}
            className="w-full bg-sera-surface border border-sera-border rounded px-2 py-1 text-xs text-sera-text focus:outline-none focus:border-sera-accent"
          >
            {!agents?.length && <option value="">No agents</option>}
            {agents?.map((a) => (
              <option key={a.name} value={a.name}>
                {a.display_name ?? a.name}
              </option>
            ))}
          </select>
        )}
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto">
        {sessions.length === 0 ? (
          <div className="px-3 py-6 text-center">
            <MessageSquare size={20} className="text-sera-text-muted mx-auto mb-2" />
            <p className="text-[11px] text-sera-text-muted">No sessions yet</p>
          </div>
        ) : (
          <div className="py-2">
            {Object.entries(groupedSessions).map(([agentName, agentSessions]) => (
              <div key={agentName} className="mb-4">
                <div className="px-3 py-1 mb-1">
                  <span className="text-[10px] font-bold uppercase tracking-wider text-sera-text-muted flex items-center gap-1.5">
                    <Bot size={10} />
                    {agentName}
                  </span>
                </div>
                <div className="space-y-0.5">
                  {agentSessions.map((s) => (
                    <div
                      key={s.id}
                      role="button"
                      tabIndex={0}
                      onClick={() => onLoadSession(s.id)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' || e.key === ' ') {
                          e.preventDefault();
                          onLoadSession(s.id);
                        }
                      }}
                      className={cn(
                        'w-full text-left px-3 py-2 flex items-start gap-2 group transition-colors border-l-2 cursor-pointer',
                        sessionId === s.id
                          ? 'bg-sera-accent-soft border-sera-accent'
                          : 'hover:bg-sera-surface border-transparent'
                      )}
                    >
                      <MessageSquare
                        size={13}
                        className="text-sera-text-muted mt-0.5 flex-shrink-0"
                      />
                      <div className="flex-1 min-w-0">
                        <p className="text-xs text-sera-text truncate">{s.title}</p>
                        <p className="text-[10px] text-sera-text-muted mt-0.5">
                          {s.messageCount} messages · {new Date(s.updatedAt).toLocaleDateString()}
                        </p>
                      </div>
                      <button
                        onClick={(e) => onDeleteSession(s.id, e)}
                        className="opacity-0 group-hover:opacity-100 p-0.5 rounded text-sera-text-muted hover:text-red-400 transition-all"
                        title="Delete session"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
