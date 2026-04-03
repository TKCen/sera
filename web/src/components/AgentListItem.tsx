import React from 'react';
import { Link } from 'react-router';
import { Bot, Play, Square, Trash2, Loader2 } from 'lucide-react';
import { AgentStatusBadge } from '@/components/AgentStatusBadge';
import { Badge } from '@/components/ui/badge';
import { Tooltip } from '@/components/ui/tooltip';
import type { AgentInstance } from '@/lib/api/types';

interface AgentListItemProps {
  agent: AgentInstance;
  onStart: (e: React.MouseEvent, id: string) => void;
  onStop: (e: React.MouseEvent, id: string) => void;
  onDelete: (e: React.MouseEvent, id: string, name: string) => void;
  isStartPending?: boolean;
  isStopPending?: boolean;
  isDeletePending?: boolean;
  style?: React.CSSProperties;
}

export const AgentListItem: React.FC<AgentListItemProps> = ({
  agent,
  onStart,
  onStop,
  onDelete,
  isStartPending,
  isStopPending,
  isDeletePending,
  style,
}) => {
  return (
    <div
      role="listitem"
      className="sera-card relative flex items-center gap-4 px-4 py-3 group"
      style={style}
    >
      <div className="h-9 w-9 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
        <Bot size={16} className="text-sera-accent" />
      </div>

      <div className="flex-1 min-w-0">
        <div className="font-medium text-sm text-sera-text truncate">
          {agent.display_name ?? agent.name}
        </div>
        <div className="flex items-center gap-2 mt-0.5">
          <span className="text-xs text-sera-text-dim truncate">{agent.name}</span>
          {agent.template_ref && <Badge variant="default">{agent.template_ref}</Badge>}
          {agent.circle && <Badge variant="default">{agent.circle}</Badge>}
          {agent.sandbox_boundary && <Badge variant="accent">{agent.sandbox_boundary}</Badge>}
        </div>
      </div>

      <div className="relative z-10">
        <Tooltip content={`Status: ${agent.status}`}>
          <div className="cursor-default">
            <AgentStatusBadge agentId={agent.id} staticStatus={agent.status} />
          </div>
        </Tooltip>
      </div>

      {/* Quick actions */}
      <div className="relative z-10 flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
        <Tooltip content="Start agent">
          <button
            onClick={(e) => {
              onStart(e, agent.id);
            }}
            disabled={isStartPending}
            className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-success hover:bg-sera-success/10 transition-colors disabled:opacity-50"
            aria-label="Start agent"
          >
            {isStartPending ? <Loader2 size={13} className="animate-spin" /> : <Play size={13} />}
          </button>
        </Tooltip>
        <Tooltip content="Stop agent">
          <button
            onClick={(e) => {
              onStop(e, agent.id);
            }}
            disabled={isStopPending}
            className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-error hover:bg-sera-error/10 transition-colors disabled:opacity-50"
            aria-label="Stop agent"
          >
            {isStopPending ? <Loader2 size={13} className="animate-spin" /> : <Square size={13} />}
          </button>
        </Tooltip>
        <Tooltip content="Delete agent">
          <button
            onClick={(e) => {
              onDelete(e, agent.id, agent.name);
            }}
            disabled={isDeletePending}
            className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-error hover:bg-sera-error/10 transition-colors disabled:opacity-50"
            aria-label="Delete agent"
          >
            {isDeletePending ? (
              <Loader2 size={13} className="animate-spin" />
            ) : (
              <Trash2 size={13} />
            )}
          </button>
        </Tooltip>
      </div>

      {/* Row is clickable */}
      <Link
        to={`/agents/${agent.id}`}
        className="absolute inset-0 rounded-xl"
        aria-label={`View ${agent.name}`}
      />
    </div>
  );
};
