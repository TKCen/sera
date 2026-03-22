import { useState, useMemo } from 'react';
import { Link } from 'react-router';
import { Bot, Plus, Play, Square, Trash2, Search } from 'lucide-react';
import { toast } from 'sonner';
import { useAgents, useStartAgent, useStopAgent, useDeleteAgent } from '@/hooks/useAgents';
import { AgentStatusBadge } from '@/components/AgentStatusBadge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

// TODO: virtualise if > 100 agents

const STATUS_OPTIONS = ['all', 'running', 'stopped', 'created', 'error', 'unresponsive'] as const;

export default function AgentsPage() {
  const { data: agents, isLoading } = useAgents();
  const startAgent = useStartAgent();
  const stopAgent = useStopAgent();
  const deleteAgent = useDeleteAgent();

  const [search, setSearch] = useState('');
  const [filterStatus, setFilterStatus] = useState<string>('all');
  const [filterCircle, setFilterCircle] = useState<string>('all');
  const [agentToDelete, setAgentToDelete] = useState<{ id: string; name: string } | null>(null);

  const circles = useMemo(() => {
    if (!agents) return [];
    const set = new Set(agents.map((a) => a.circle).filter(Boolean) as string[]);
    return Array.from(set);
  }, [agents]);

  const filtered = useMemo(() => {
    if (!agents) return [];
    return agents.filter((a) => {
      const label = (a.display_name ?? a.name).toLowerCase();
      if (
        search &&
        !label.includes(search.toLowerCase()) &&
        !a.name.includes(search.toLowerCase())
      ) {
        return false;
      }
      if (filterStatus !== 'all' && a.status !== filterStatus) return false;
      if (filterCircle !== 'all' && a.circle !== filterCircle) return false;
      return true;
    });
  }, [agents, search, filterStatus, filterCircle]);

  async function handleStart(e: React.MouseEvent, id: string) {
    e.preventDefault();
    e.stopPropagation();
    try {
      await startAgent.mutateAsync(id);
      toast.success('Starting agent…');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to start');
    }
  }

  async function handleStop(e: React.MouseEvent, id: string) {
    e.preventDefault();
    e.stopPropagation();
    try {
      await stopAgent.mutateAsync(id);
      toast.success('Stopping agent…');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to stop');
    }
  }

  async function handleDelete(e: React.MouseEvent, id: string, name: string) {
    e.preventDefault();
    e.stopPropagation();
    setAgentToDelete({ id, name });
  }

  async function confirmDelete() {
    if (!agentToDelete) return;
    try {
      await deleteAgent.mutateAsync(agentToDelete.id);
      toast.success(`Deleted ${agentToDelete.name}`);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to delete');
    } finally {
      setAgentToDelete(null);
    }
  }

  return (
    <main className="p-6">
      <header className="sera-page-header">
        <h1 className="sera-page-title">Agents</h1>
        <Button asChild size="sm">
          <Link to="/agents/new">
            <Plus size={14} />
            New Agent
          </Link>
        </Button>
      </header>

      {/* Filters */}
      <section aria-label="Filters" className="flex items-center gap-3 mb-4">
        <div className="relative flex-1 max-w-xs">
          <Search
            size={13}
            className="absolute left-2.5 top-1/2 -translate-y-1/2 text-sera-text-dim pointer-events-none"
          />
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
            aria-label="Filter by circle"
          >
            <option value="all">All circles</option>
            {circles.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        )}

        <nav aria-label="Filter by status" className="flex items-center gap-1">
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
        </nav>
      </section>

      {isLoading ? (
        <div className="space-y-3" role="status" aria-live="polite" aria-label="Loading agents">
          {[1, 2, 3].map((i) => (
            <div key={i} className="sera-card flex items-center gap-4 px-4 py-3">
              <Skeleton className="h-9 w-9 rounded-lg flex-shrink-0" />
              <div className="flex-1 space-y-2">
                <Skeleton className="h-4 w-48" />
                <div className="flex gap-2">
                  <Skeleton className="h-3 w-24" />
                  <Skeleton className="h-3 w-16" />
                </div>
              </div>
              <Skeleton className="h-5 w-20 rounded-full" />
            </div>
          ))}
        </div>
      ) : !agents?.length ? (
        <div role="status" aria-live="polite">
          <EmptyState
            icon={<Bot size={24} />}
            title="No agents yet"
            description="Create your first agent from a template to get started."
            action={
              <Button asChild size="sm">
                <Link to="/agents/new">Create Agent</Link>
              </Button>
            }
          />
        </div>
      ) : filtered.length === 0 ? (
        <p
          className="text-sm text-sera-text-muted text-center py-12"
          role="status"
          aria-live="polite"
        >
          No agents match your filters.
        </p>
      ) : (
        <ul className="space-y-2" aria-label="Agents list">
          {filtered.map((agent) => (
            <li
              key={agent.id}
              className="sera-card relative flex items-center gap-4 px-4 py-3 group"
            >
              <div className="h-9 w-9 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
                <Bot size={16} className="text-sera-accent" />
              </div>

              <div className="flex-1 min-w-0">
                <div className="font-medium text-sm text-sera-text truncate relative z-10">
                  <Link
                    to={`/agents/${agent.id}`}
                    className="hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sera-accent focus-visible:ring-offset-2 rounded"
                  >
                    {agent.display_name ?? agent.name}
                  </Link>
                </div>
                <div className="flex items-center gap-2 mt-0.5">
                  <span className="text-xs text-sera-text-dim truncate">{agent.name}</span>
                  {agent.template_ref && <Badge variant="default">{agent.template_ref}</Badge>}
                  {agent.circle && <Badge variant="default">{agent.circle}</Badge>}
                  {agent.sandbox_boundary && (
                    <Badge variant="accent">{agent.sandbox_boundary}</Badge>
                  )}
                </div>
              </div>

              <div className="relative z-10">
                <AgentStatusBadge agentId={agent.id} staticStatus={agent.status} />
              </div>

              {/* Quick actions */}
              <div className="relative z-10 flex items-center gap-1 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity">
                <button
                  onClick={(e) => {
                    void handleStart(e, agent.id);
                  }}
                  disabled={startAgent.isPending}
                  className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-success hover:bg-sera-success/10 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sera-accent"
                  title="Start"
                  aria-label={`Start ${agent.name}`}
                >
                  <Play size={13} />
                </button>
                <button
                  onClick={(e) => {
                    void handleStop(e, agent.id);
                  }}
                  disabled={stopAgent.isPending}
                  className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-error hover:bg-sera-error/10 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sera-accent"
                  title="Stop"
                  aria-label={`Stop ${agent.name}`}
                >
                  <Square size={13} />
                </button>
                <button
                  onClick={(e) => {
                    void handleDelete(e, agent.id, agent.name);
                  }}
                  disabled={deleteAgent.isPending}
                  className="p-1.5 rounded-md text-sera-text-muted hover:text-sera-error hover:bg-sera-error/10 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sera-accent"
                  title="Delete"
                  aria-label={`Delete ${agent.name}`}
                >
                  <Trash2 size={13} />
                </button>
              </div>

              {/* Row is clickable (presentational overlay) */}
              <Link
                to={`/agents/${agent.id}`}
                className="absolute inset-0 rounded-xl"
                aria-hidden="true"
                tabIndex={-1}
              />
            </li>
          ))}
        </ul>
      )}
      <Dialog open={!!agentToDelete} onOpenChange={(open) => !open && setAgentToDelete(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Agent</DialogTitle>
            <DialogDescription>
              Delete agent "{agentToDelete?.name}"? This will stop its container and remove the
              instance permanently.
            </DialogDescription>
          </DialogHeader>
          <div className="flex justify-end gap-3 mt-4">
            <Button variant="outline" onClick={() => setAgentToDelete(null)}>
              Cancel
            </Button>
            <Button variant="danger" onClick={confirmDelete} disabled={deleteAgent.isPending}>
              {deleteAgent.isPending ? 'Deleting...' : 'Delete'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </main>
  );
}
