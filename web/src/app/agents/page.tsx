import { useState, useMemo, useRef, useEffect } from 'react';
import { useNavigate } from 'react-router';
import { Bot, Plus, Search } from 'lucide-react';
import { Virtuoso } from 'react-virtuoso';
import { toast } from 'sonner';
import { useAgents, useStartAgent, useStopAgent, useDeleteAgent } from '@/hooks/useAgents';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { AgentForm } from '@/components/AgentForm';
import { AgentListItem } from '@/components/AgentListItem';

const VIRTUALIZATION_THRESHOLD = 100;

const STATUS_OPTIONS = ['all', 'running', 'stopped', 'created', 'error', 'unresponsive'] as const;

function AgentsPageContent() {
  const navigate = useNavigate();
  const { data: agents, isLoading, isError, refetch } = useAgents();
  const startAgent = useStartAgent();
  const stopAgent = useStopAgent();
  const deleteAgent = useDeleteAgent();

  const [search, setSearch] = useState('');
  const [filterStatus, setFilterStatus] = useState<string>('all');
  const [filterCircle, setFilterCircle] = useState<string>('all');
  const [confirmDelete, setConfirmDelete] = useState<{ id: string; name: string } | null>(null);
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [liveMessage, setLiveMessage] = useState<string>('');
  const titleRef = useRef<HTMLHeadingElement>(null);

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
    setConfirmDelete({ id, name });
  }

  useEffect(() => {
    if (liveMessage) {
      const timer = setTimeout(() => setLiveMessage(''), 3000);
      return () => clearTimeout(timer);
    }
  }, [liveMessage]);

  return (
    <main className="p-6">
      {/* Visually hidden live region for screen readers */}
      <div className="sr-only" aria-live="polite" aria-atomic="true">
        {liveMessage}
      </div>

      <header className="sera-page-header">
        <h1 ref={titleRef} tabIndex={-1} className="sera-page-title outline-none">
          Agents
        </h1>
        <Button size="sm" onClick={() => setIsCreateDialogOpen(true)}>
          <Plus size={14} />
          New Agent
        </Button>
      </header>

      {/* Filters */}
      <section aria-label="Filters" className="flex items-center gap-3 mb-4">
        <div role="search" className="relative flex-1 max-w-xs">
          <Search
            size={13}
            className="absolute left-2.5 top-1/2 -translate-y-1/2 text-sera-text-dim pointer-events-none"
          />
          <Input
            id="search-agents"
            aria-label="Search agents"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search agents…"
            className="pl-8"
          />
        </div>

        {circles.length > 0 && (
          <div>
            <label htmlFor="circle-filter" className="sr-only">
              Filter by circle
            </label>
            <select
              id="circle-filter"
              value={filterCircle}
              onChange={(e) => setFilterCircle(e.target.value)}
              className="sera-input h-9 py-0 w-auto text-xs"
            >
              <option value="all">All circles</option>
              {circles.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </div>
        )}

        <nav
          aria-label="Filter by status"
          className="flex items-center gap-1 p-1 bg-sera-surface rounded-lg border border-sera-border"
        >
          {STATUS_OPTIONS.map((s) => (
            <button
              key={s}
              onClick={() => {
                setFilterStatus(s);
                setLiveMessage(`Filtering by ${s} status`);
              }}
              aria-pressed={filterStatus === s}
              className={
                filterStatus === s
                  ? 'px-3 py-1 rounded-md text-[11px] font-semibold uppercase tracking-wider bg-sera-accent-soft text-sera-accent'
                  : 'px-3 py-1 rounded-md text-[11px] font-semibold uppercase tracking-wider text-sera-text-muted hover:bg-sera-surface-hover hover:text-sera-text transition-colors'
              }
            >
              {s === 'all' ? 'All' : s}
            </button>
          ))}
        </nav>
      </section>

      {isLoading ? (
        <div aria-label="Loading agents" role="status" className="space-y-3">
          {[1, 2, 3].map((i) => (
            <div key={i}>
              <Skeleton className="h-16 rounded-xl" />
            </div>
          ))}
        </div>
      ) : isError ? (
        <div className="flex flex-col items-center justify-center p-8 border border-sera-border rounded-xl bg-sera-surface mt-4">
          <p className="text-sera-error mb-4">Failed to load agents.</p>
          <Button onClick={() => refetch()} variant="outline">
            Retry
          </Button>
        </div>
      ) : !agents?.length ? (
        <EmptyState
          icon={<Bot size={24} />}
          title="No agents yet"
          description="Create your first agent from a template to get started."
          action={
            <Button size="sm" onClick={() => setIsCreateDialogOpen(true)}>
              Create Agent
            </Button>
          }
        />
      ) : filtered.length === 0 ? (
        <p className="text-sm text-sera-text-muted text-center py-12">
          No agents match your filters.
        </p>
      ) : filtered.length > VIRTUALIZATION_THRESHOLD ? (
        <Virtuoso
          useWindowScroll
          data={filtered}
          itemContent={(_index, agent) => (
            <div className="pb-2">
              <AgentListItem
                agent={agent}
                onStart={handleStart}
                onStop={handleStop}
                onDelete={handleDelete}
                isStartPending={startAgent.isPending}
                isStopPending={stopAgent.isPending}
                isDeletePending={deleteAgent.isPending}
              />
            </div>
          )}
        />
      ) : (
        <div role="list" aria-label="Agents list" aria-live="polite" className="space-y-2">
          {filtered.map((agent) => (
            <AgentListItem
              key={agent.id}
              agent={agent}
              onStart={handleStart}
              onStop={handleStop}
              onDelete={handleDelete}
              isStartPending={startAgent.isPending}
              isStopPending={stopAgent.isPending}
              isDeletePending={deleteAgent.isPending}
            />
          ))}
        </div>
      )}

      {/* Create agent dialog */}
      <Dialog open={isCreateDialogOpen} onOpenChange={setIsCreateDialogOpen}>
        <DialogContent className="max-w-2xl max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Create Agent</DialogTitle>
            <DialogDescription>Deploy a new agent instance from a template.</DialogDescription>
          </DialogHeader>
          <div className="mt-4">
            <AgentForm
              onSuccess={(id) => {
                setIsCreateDialogOpen(false);
                void navigate(`/agents/${id}`);
              }}
              onCancel={() => setIsCreateDialogOpen(false)}
            />
          </div>
        </DialogContent>
      </Dialog>

      {/* Confirmation dialog */}
      <Dialog
        open={confirmDelete !== null}
        onOpenChange={(open: boolean) => !open && setConfirmDelete(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Agent</DialogTitle>
            <DialogDescription>
              Delete agent "{confirmDelete?.name}"? This will stop its container and remove the
              instance permanently.
            </DialogDescription>
          </DialogHeader>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button
              size="sm"
              variant="danger"
              disabled={deleteAgent.isPending}
              onClick={() => {
                if (!confirmDelete) return;
                void deleteAgent
                  .mutateAsync(confirmDelete.id)
                  .then(() => {
                    toast.success(`Deleted ${confirmDelete.name}`);
                    setLiveMessage(`Deleted agent ${confirmDelete.name}`);
                    setConfirmDelete(null);
                    titleRef.current?.focus();
                  })
                  .catch((err: unknown) => {
                    toast.error(err instanceof Error ? err.message : 'Failed to delete');
                  });
              }}
            >
              {deleteAgent.isPending ? 'Deleting…' : 'Delete'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </main>
  );
}

export default function AgentsPage() {
  return (
    <ErrorBoundary fallbackMessage="The agents page encountered an error.">
      <AgentsPageContent />
    </ErrorBoundary>
  );
}
