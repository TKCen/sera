import { useState, useMemo } from 'react';
import { Link } from 'react-router';
import { Bot, Plus, Play, Square, ExternalLink, Search } from 'lucide-react';
import { toast } from 'sonner';
import { useAgents, useStartAgent, useStopAgent } from '@/hooks/useAgents';
import { AgentStatusBadge } from '@/components/AgentStatusBadge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';

// TODO: virtualise if > 100 agents

const STATUS_OPTIONS = ['all', 'running', 'stopped', 'error', 'unresponsive'] as const;

export default function AgentsPage() {
  const { data: agents, isLoading } = useAgents();
  const startAgent = useStartAgent();
  const stopAgent = useStopAgent();

  const [search, setSearch] = useState('');
  const [filterStatus, setFilterStatus] = useState<string>('all');
  const [filterCircle, setFilterCircle] = useState<string>('all');

  const circles = useMemo(() => {
    if (!agents) return [];
    const set = new Set(agents.map((a) => a.metadata.circle).filter(Boolean) as string[]);
    return Array.from(set);
  }, [agents]);

  const filtered = useMemo(() => {
    if (!agents) return [];
    return agents.filter((a) => {
      const name = (a.metadata.displayName ?? a.metadata.name).toLowerCase();
      if (search && !name.includes(search.toLowerCase()) && !a.metadata.name.includes(search.toLowerCase())) {
        return false;
      }
      if (filterCircle !== 'all' && a.metadata.circle !== filterCircle) return false;
      return true;
    });
  }, [agents, search, filterCircle]);

  async function handleStart(e: React.MouseEvent, name: string) {
    e.preventDefault();
    e.stopPropagation();
    try {
      await startAgent.mutateAsync(name);
      toast.success(`Starting ${name}…`);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to start');
    }
  }

  async function handleStop(e: React.MouseEvent, name: string) {
    e.preventDefault();
    e.stopPropagation();
    try {
      await stopAgent.mutateAsync(name);
      toast.success(`Stopping ${name}…`);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to stop');
    }
  }

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">Agents</h1>
        <Button asChild size="sm">
          <Link to="/agents/new">
            <Plus size={14} />
            New Agent
          </Link>
        </Button>
      </div>

      {/* Filters */}
      <div className="flex items-center gap-3 mb-4">
        <div className="relative flex-1 max-w-xs">
          <Search size={13} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-sera-text-dim pointer-events-none" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search agents…"
            className="pl-8"
          />
        </div>

        {circles.length > 0 && (
          <select
            value={filterCircle}
            onChange={(e) => setFilterCircle(e.target.value)}
            className="sera-input h-9 py-0 w-auto text-xs"
          >
            <option value="all">All circles</option>
            {circles.map((c) => (
              <option key={c} value={c}>{c}</option>
            ))}
          </select>
        )}

        <div className="flex items-center gap-1">
          {STATUS_OPTIONS.map((s) => (
            <button
              key={s}
              onClick={() => setFilterStatus(s)}
              className={
                filterStatus === s
                  ? 'px-2.5 py-1 rounded-md text-xs font-medium bg-sera-accent-soft text-sera-accent'
                  : 'px-2.5 py-1 rounded-md text-xs font-medium text-sera-text-muted hover:bg-sera-surface-hover transition-colors'
              }
            >
              {s === 'all' ? 'All' : s.charAt(0).toUpperCase() + s.slice(1)}
            </button>
          ))}
        </div>
      </div>

      {isLoading ? (
        <div className="space-y-3">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-16 rounded-xl" />
          ))}
        </div>
      ) : !agents?.length ? (
        <EmptyState
          icon={<Bot size={24} />}
          title="No agents yet"
          description="Create your first agent to get started."
          action={
            <Button asChild size="sm">
              <Link to="/agents/new">Create Agent</Link>
            </Button>
          }
        />
      ) : filtered.length === 0 ? (
        <p className="text-sm text-sera-text-muted text-center py-12">No agents match your filters.</p>
      ) : (
        <div className="space-y-2">
          {filtered.map((agent) => {
            const id = agent.metadata.name;
            const tier = agent.spec?.sandboxBoundary ?? '';

            return (
              <div
                key={id}
                className="sera-card relative flex items-center gap-4 px-4 py-3 group"
              >
                <div className="h-9 w-9 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
                  <Bot size={16} className="text-sera-accent" />
                </div>

                <div className="flex-1 min-w-0">
                  <div className="font-medium text-sm text-sera-text truncate">
                    {agent.metadata.displayName ?? id}
                  </div>
                  <div className="flex items-center gap-2 mt-0.5">
                    <span className="text-xs text-sera-text-dim truncate">{id}</span>
                    {agent.metadata.circle && (
                      <Badge variant="default">{agent.metadata.circle}</Badge>
                    )}
                    {tier && (
                      <Badge variant="accent">{tier}</Badge>
                    )}
                  </div>
                </div>

                <AgentStatusBadge agentId={id} staticStatus={undefined} />

                {/* Quick actions */}
                <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button
                    onClick={(e) => { void handleStart(e, id); }}
                    disabled={startAgent.isPending}
                    className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-success hover:bg-sera-success/10 transition-colors"
                    title="Start"
                  >
                    <Play size={13} />
                  </button>
                  <button
                    onClick={(e) => { void handleStop(e, id); }}
                    disabled={stopAgent.isPending}
                    className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-error hover:bg-sera-error/10 transition-colors"
                    title="Stop"
                  >
                    <Square size={13} />
                  </button>
                  <Link
                    to={`/agents/${id}`}
                    className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-accent hover:bg-sera-accent-soft transition-colors"
                    title="View"
                  >
                    <ExternalLink size={13} />
                  </Link>
                </div>

                {/* Row is clickable */}
                <Link
                  to={`/agents/${id}`}
                  className="absolute inset-0 rounded-xl"
                  aria-label={`View ${id}`}
                />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
