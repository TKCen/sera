import React, { useState, useRef, useEffect } from 'react';
import { Plus, Bot, MessageSquare, Trash2, ChevronDown, Pencil, Check, X } from 'lucide-react';
import { cn } from '@/lib/utils';

interface SessionInfo {
  id: string;
  agentName: string;
  agentInstanceId?: string | null;
  title: string;
  messageCount: number;
  createdAt: string;
  updatedAt: string;
}

interface AgentInfo {
  id: string;
  name: string;
  display_name?: string | null;
  status?: string | null;
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
  onRenameSession: (id: string, title: string) => void;
  onRefetchAgents: () => void;
}

function statusColor(status?: string | null): string {
  switch (status) {
    case 'running':
      return 'bg-sera-success';
    case 'stopped':
      return 'bg-sera-text-dim';
    case 'error':
      return 'bg-sera-error';
    default:
      return 'bg-sera-text-muted';
  }
}

function AgentDropdown({
  agents,
  selectedAgent,
  onAgentChange,
}: {
  agents: AgentInfo[];
  selectedAgent: string;
  onAgentChange: (name: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selected = agents.find((a) => a.name === selectedAgent);

  useEffect(() => {
    function onClickOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener('mousedown', onClickOutside);
    return () => document.removeEventListener('mousedown', onClickOutside);
  }, []);

  if (!agents.length) {
    return <span className="text-xs text-sera-text-muted">No agents</span>;
  }

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        className="w-full flex items-center gap-2 bg-sera-surface border border-sera-border rounded px-2 py-1.5 text-xs text-sera-text hover:border-sera-accent transition-colors"
      >
        <span className={cn('w-2 h-2 rounded-full flex-shrink-0', statusColor(selected?.status))} />
        <span className="flex-1 text-left truncate">
          {selected?.display_name ?? selected?.name ?? selectedAgent}
        </span>
        <ChevronDown
          size={12}
          className={cn('text-sera-text-muted transition-transform', open && 'rotate-180')}
        />
      </button>
      {open && (
        <div className="absolute z-50 top-full left-0 right-0 mt-1 bg-sera-surface border border-sera-border rounded shadow-lg max-h-48 overflow-y-auto">
          {agents.map((a) => (
            <button
              key={a.name}
              onClick={() => {
                onAgentChange(a.name);
                setOpen(false);
              }}
              className={cn(
                'w-full flex items-center gap-2 px-2 py-1.5 text-xs text-left hover:bg-sera-surface-hover transition-colors',
                a.name === selectedAgent && 'bg-sera-accent-soft text-sera-accent'
              )}
            >
              <span className={cn('w-2 h-2 rounded-full flex-shrink-0', statusColor(a.status))} />
              <span className="truncate">{a.display_name ?? a.name}</span>
              {a.status && (
                <span className="ml-auto text-[10px] text-sera-text-dim">{a.status}</span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function SessionItem({
  session,
  isActive,
  onLoad,
  onDelete,
  onRename,
}: {
  session: SessionInfo;
  isActive: boolean;
  onLoad: () => void;
  onDelete: (e: React.MouseEvent) => void;
  onRename: (title: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState(session.title);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editing]);

  const commitRename = () => {
    const trimmed = editValue.trim();
    if (trimmed && trimmed !== session.title) {
      onRename(trimmed);
    }
    setEditing(false);
  };

  const cancelRename = () => {
    setEditValue(session.title);
    setEditing(false);
  };

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => !editing && onLoad()}
      onKeyDown={(e) => {
        if (!editing && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault();
          onLoad();
        }
      }}
      onDoubleClick={(e) => {
        e.stopPropagation();
        setEditValue(session.title);
        setEditing(true);
      }}
      className={cn(
        'w-full text-left px-3 py-2 flex items-start gap-2 group transition-colors border-l-2 cursor-pointer',
        isActive
          ? 'bg-sera-accent-soft border-sera-accent'
          : 'hover:bg-sera-surface border-transparent'
      )}
    >
      <MessageSquare size={13} className="text-sera-text-muted mt-0.5 flex-shrink-0" />
      <div className="flex-1 min-w-0">
        {editing ? (
          <div className="flex items-center gap-1">
            <input
              ref={inputRef}
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitRename();
                if (e.key === 'Escape') cancelRename();
                e.stopPropagation();
              }}
              onClick={(e) => e.stopPropagation()}
              className="text-xs bg-sera-surface border border-sera-border rounded px-1 py-0.5 w-full text-sera-text outline-none focus:border-sera-accent"
            />
            <button
              onClick={(e) => {
                e.stopPropagation();
                commitRename();
              }}
              className="p-0.5 text-sera-success hover:text-green-400"
              title="Save"
            >
              <Check size={11} />
            </button>
            <button
              onClick={(e) => {
                e.stopPropagation();
                cancelRename();
              }}
              className="p-0.5 text-sera-text-muted hover:text-red-400"
              title="Cancel"
            >
              <X size={11} />
            </button>
          </div>
        ) : (
          <>
            <p className="text-xs text-sera-text truncate">{session.title}</p>
            <p className="text-[10px] text-sera-text-muted mt-0.5">
              {session.messageCount} messages · {new Date(session.updatedAt).toLocaleDateString()}
            </p>
          </>
        )}
      </div>
      {!editing && (
        <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-all">
          <button
            onClick={(e) => {
              e.stopPropagation();
              setEditValue(session.title);
              setEditing(true);
            }}
            className="p-0.5 rounded text-sera-text-muted hover:text-sera-accent"
            title="Rename session"
          >
            <Pencil size={11} />
          </button>
          <button
            onClick={onDelete}
            className="p-0.5 rounded text-sera-text-muted hover:text-red-400"
            title="Delete session"
          >
            <Trash2 size={12} />
          </button>
        </div>
      )}
    </div>
  );
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
  onRenameSession,
  onRefetchAgents,
}: ChatSidebarProps) {
  // Resolve agent UUIDs to display names when possible
  const resolveAgentName = (session: SessionInfo): string => {
    const raw = session.agentName || 'Unknown Agent';
    // If it looks like a UUID, try to resolve via the agents list
    if (/^[0-9a-f-]{36}$/i.test(raw) && agents) {
      const match =
        agents.find((a) => a.id === raw) ?? agents.find((a) => a.id === session.agentInstanceId);
      if (match) return match.display_name ?? match.name;
    }
    return raw;
  };

  const groupedSessions = sessions.reduce<Record<string, SessionInfo[]>>((acc, s) => {
    const key = resolveAgentName(s);
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
          <AgentDropdown
            agents={agents ?? []}
            selectedAgent={selectedAgent}
            onAgentChange={onAgentChange}
          />
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
                    <SessionItem
                      key={s.id}
                      session={s}
                      isActive={sessionId === s.id}
                      onLoad={() => onLoadSession(s.id)}
                      onDelete={(e) => onDeleteSession(s.id, e)}
                      onRename={(title) => onRenameSession(s.id, title)}
                    />
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
