import { useMemo } from 'react';
import { Globe, Users } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { CircleSummary, AgentInstance } from '@/lib/api/types';
import type { IntrospectionView } from '@/hooks/useIntrospection';

interface IntrospectionSidebarProps {
  circles: CircleSummary[];
  agents: AgentInstance[];
  activeView: IntrospectionView;
  onViewChange: (view: IntrospectionView) => void;
}

function getStatusColor(status?: string): string {
  switch (status) {
    case 'running':
    case 'active':
      return 'bg-sera-success';
    case 'error':
    case 'unresponsive':
      return 'bg-sera-error';
    default:
      return 'bg-sera-text-dim';
  }
}

export function IntrospectionSidebar({
  circles,
  agents,
  activeView,
  onViewChange,
}: IntrospectionSidebarProps) {
  // Count online agents per circle
  const circleAgentCounts = useMemo(() => {
    const map = new Map<string, { online: number; total: number }>();
    circles.forEach((circle) => {
      const circleAgents = agents.filter((a) => a.circle === circle.name);
      const online = circleAgents.filter((a) => a.status === 'running').length;
      map.set(circle.name, { online, total: circleAgents.length });
    });
    return map;
  }, [circles, agents]);

  // Sort agents: running first, then stopped
  const sortedAgents = useMemo(() => {
    return [...agents].sort((a, b) => {
      const aRunning = a.status === 'running' ? 0 : 1;
      const bRunning = b.status === 'running' ? 0 : 1;
      if (aRunning !== bRunning) return aRunning - bRunning;
      return a.name.localeCompare(b.name);
    });
  }, [agents]);

  const isGlobalActive = activeView.kind === 'global';
  const activeCircle = activeView.kind === 'circle' ? activeView.circleId : null;
  const activeAgent = activeView.kind === 'agent' ? activeView.agentId : null;

  return (
    <div className="w-64 border-r border-sera-border bg-sera-surface flex flex-col overflow-hidden">
      {/* Global Feed button */}
      <div className="p-3 border-b border-sera-border">
        <button
          onClick={() => onViewChange({ kind: 'global' })}
          className={cn(
            'w-full flex items-center gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors',
            isGlobalActive
              ? 'bg-sera-accent-soft text-sera-accent'
              : 'text-sera-text hover:bg-sera-surface-hover'
          )}
        >
          <Globe size={16} />
          Global Feed
        </button>
      </div>

      {/* Scrollable content */}
      <div className="flex-1 overflow-y-auto">
        {/* Circles section */}
        {circles.length > 0 && (
          <div className="space-y-2">
            <div className="px-4 pt-3 pb-2">
              <h3 className="text-xs font-semibold text-sera-text-muted uppercase tracking-wide">
                Circles
              </h3>
            </div>
            {circles.map((circle) => {
              const counts = circleAgentCounts.get(circle.name);
              const isActive = activeCircle === circle.name;

              // Get agent IDs for this circle
              const circleAgents = agents.filter((a) => a.circle === circle.name);

              return (
                <button
                  key={circle.name}
                  onClick={() =>
                    onViewChange({
                      kind: 'circle',
                      circleId: circle.name,
                      agentIds: circleAgents.map((a) => a.id),
                    })
                  }
                  className={cn(
                    'w-full text-left px-3 py-2 mx-2 rounded-lg text-sm transition-colors flex items-center justify-between',
                    isActive
                      ? 'bg-sera-accent-soft text-sera-accent'
                      : 'text-sera-text hover:bg-sera-surface-hover'
                  )}
                >
                  <div className="flex items-center gap-2">
                    <Users size={14} />
                    <span className="truncate">{circle.displayName ?? circle.name}</span>
                  </div>
                  {counts && (
                    <span className="text-xs whitespace-nowrap ml-2 opacity-70">
                      {counts.online}/{counts.total}
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        )}

        {/* Agents section */}
        {agents.length > 0 && (
          <div className="space-y-2">
            <div className="px-4 pt-3 pb-2">
              <h3 className="text-xs font-semibold text-sera-text-muted uppercase tracking-wide">
                Agents
              </h3>
            </div>
            {sortedAgents.map((agent) => {
              const isActive = activeAgent === agent.id;

              return (
                <button
                  key={agent.id}
                  onClick={() =>
                    onViewChange({
                      kind: 'agent',
                      agentId: agent.id,
                      agentName: agent.display_name ?? agent.name,
                    })
                  }
                  className={cn(
                    'w-full text-left px-3 py-2 mx-2 rounded-lg text-sm transition-colors flex items-center gap-2',
                    isActive
                      ? 'bg-sera-accent-soft text-sera-accent'
                      : 'text-sera-text hover:bg-sera-surface-hover'
                  )}
                >
                  {/* Status indicator */}
                  <div
                    className={cn(
                      'w-2 h-2 rounded-full flex-shrink-0',
                      getStatusColor(agent.status)
                    )}
                  />
                  <span className="truncate text-sm">{agent.display_name ?? agent.name}</span>
                </button>
              );
            })}
          </div>
        )}

        {/* Empty state */}
        {circles.length === 0 && agents.length === 0 && (
          <div className="p-4 text-center text-sera-text-dim text-sm">
            <p>No circles or agents yet</p>
          </div>
        )}
      </div>
    </div>
  );
}
