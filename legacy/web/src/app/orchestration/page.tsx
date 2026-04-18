import { useState, useMemo } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { ChevronDown, ChevronRight, GitBranch, Plus, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import * as orchestrationApi from '@/lib/api/orchestration';
import type { AgentTask } from '@/lib/api/types';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ErrorBoundary } from '@/components/ErrorBoundary';

// ── Query keys ───────────────────────────────────────────────────────────────

const orchestrationKeys = {
  bridges: ['orchestration', 'bridges'] as const,
  tasks: (agentId: string, status?: string) => ['orchestration', 'tasks', agentId, status] as const,
  queueDepth: (agentId: string) => ['orchestration', 'queue-depth', agentId] as const,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

const BRIDGE_NAMES: Record<string, string> = {
  'omc-bridge': 'OMC Bridge',
  'omo-bridge': 'OMO Bridge',
  'omx-bridge': 'OMX Bridge',
  'gemini-bridge': 'Gemini Bridge',
};

const TOOL_OPTIONS = ['All', 'OMC', 'OMO', 'OMX', 'Gemini', 'Auto'] as const;
const STATUS_OPTIONS = ['all', 'queued', 'running', 'completed', 'failed'] as const;

type StatusFilter = (typeof STATUS_OPTIONS)[number];

function statusBadgeClass(status: AgentTask['status']): string {
  switch (status) {
    case 'queued':
      return 'bg-sera-surface text-sera-text-muted border border-sera-border';
    case 'running':
      return 'bg-blue-500/15 text-blue-400 border border-blue-500/30';
    case 'completed':
      return 'bg-sera-success/15 text-sera-success border border-sera-success/30';
    case 'failed':
      return 'bg-sera-error/15 text-sera-error border border-sera-error/30';
    default:
      return 'bg-sera-surface text-sera-text-muted border border-sera-border';
  }
}

function agentStatusBadgeClass(status: string): string {
  if (status === 'running')
    return 'bg-sera-success/15 text-sera-success border border-sera-success/30';
  if (status === 'error') return 'bg-sera-error/15 text-sera-error border border-sera-error/30';
  return 'bg-sera-warning/15 text-sera-warning border border-sera-warning/30';
}

function formatDuration(task: AgentTask): string {
  const start = task.startedAt ? new Date(task.startedAt).getTime() : null;
  const end = task.completedAt ? new Date(task.completedAt).getTime() : null;
  if (!start) return '—';
  const elapsed = (end ?? Date.now()) - start;
  const s = Math.floor(elapsed / 1000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m ${s % 60}s`;
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

// ── Bridge Status Card ───────────────────────────────────────────────────────

function BridgeCard({ agent }: { agent: orchestrationApi.BridgeAgent }) {
  const { data: depth } = useQuery({
    queryKey: orchestrationKeys.queueDepth(agent.id),
    queryFn: () => orchestrationApi.getQueueDepth(agent.id),
    refetchInterval: 15000,
  });

  const displayName = BRIDGE_NAMES[agent.name] ?? agent.display_name ?? agent.name;

  return (
    <Card className="sera-card-static">
      <CardHeader className="pb-2 pt-4 px-4">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-medium text-sera-text">{displayName}</CardTitle>
          <Badge className={`text-[10px] px-1.5 py-0.5 ${agentStatusBadgeClass(agent.status)}`}>
            {agent.status}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="px-4 pb-4">
        <p className="text-xs text-sera-text-dim">
          Queue:{' '}
          <span className="text-sera-text font-medium">{depth === undefined ? '…' : depth}</span>
        </p>
        <p className="text-[10px] text-sera-text-dim mt-1 truncate">{agent.name}</p>
      </CardContent>
    </Card>
  );
}

// ── Task Row ─────────────────────────────────────────────────────────────────

function TaskRow({ task, agentName }: { task: AgentTask; agentName: string }) {
  const [expanded, setExpanded] = useState(false);

  const shortId = task.id.slice(0, 8);
  const prompt =
    typeof task.task === 'string'
      ? task.task.slice(0, 80) + (task.task.length > 80 ? '…' : '')
      : '—';

  return (
    <>
      <tr
        className="border-b border-sera-border/50 hover:bg-sera-surface-hover cursor-pointer transition-colors"
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
      >
        <td className="py-3 px-4 font-mono text-xs text-sera-text-dim">{shortId}</td>
        <td className="py-3 px-4 text-xs text-sera-text max-w-xs truncate">{prompt}</td>
        <td className="py-3 px-4 text-xs text-sera-text-muted">{agentName}</td>
        <td className="py-3 px-4">
          <span
            className={`text-[10px] px-1.5 py-0.5 rounded-full font-medium ${statusBadgeClass(task.status)}`}
          >
            {task.status}
          </span>
        </td>
        <td className="py-3 px-4 text-xs text-sera-text-muted">{task.priority}</td>
        <td className="py-3 px-4 text-xs text-sera-text-dim">{formatDate(task.createdAt)}</td>
        <td className="py-3 px-4 text-xs text-sera-text-dim">{formatDuration(task)}</td>
        <td className="py-3 px-4 text-sera-text-dim">
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </td>
      </tr>
      {expanded && (
        <tr className="border-b border-sera-border/50 bg-sera-surface/50">
          <td colSpan={8} className="py-4 px-6">
            <div className="space-y-3">
              <div>
                <p className="text-[10px] uppercase tracking-wider text-sera-text-dim mb-1">
                  Full Prompt
                </p>
                <pre className="text-xs text-sera-text bg-sera-bg rounded-lg p-3 whitespace-pre-wrap break-words font-mono max-h-48 overflow-y-auto">
                  {typeof task.task === 'string' ? task.task : JSON.stringify(task.task, null, 2)}
                </pre>
              </div>
              {task.result !== undefined && task.result !== null && (
                <div>
                  <p className="text-[10px] uppercase tracking-wider text-sera-text-dim mb-1">
                    Result
                  </p>
                  <pre className="text-xs text-sera-text bg-sera-bg rounded-lg p-3 whitespace-pre-wrap break-words font-mono max-h-48 overflow-y-auto">
                    {typeof task.result === 'string'
                      ? task.result
                      : JSON.stringify(task.result, null, 2)}
                  </pre>
                </div>
              )}
              {task.error && (
                <div>
                  <p className="text-[10px] uppercase tracking-wider text-sera-error mb-1">Error</p>
                  <pre className="text-xs text-sera-error bg-sera-error/5 rounded-lg p-3 whitespace-pre-wrap break-words font-mono">
                    {task.error}
                  </pre>
                </div>
              )}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

// ── Create Task Form ─────────────────────────────────────────────────────────

const TOOL_TO_BRIDGE: Record<string, string> = {
  OMC: 'omc-bridge',
  OMO: 'omo-bridge',
  OMX: 'omx-bridge',
  Gemini: 'gemini-bridge',
};

function CreateTaskForm({
  bridges,
  onCreated,
}: {
  bridges: orchestrationApi.BridgeAgent[];
  onCreated: () => void;
}) {
  const qc = useQueryClient();
  const [prompt, setPrompt] = useState('');
  const [tool, setTool] = useState<string>('Auto');
  const [priority, setPriority] = useState<1 | 2 | 3>(2);

  const mutation = useMutation({
    mutationFn: ({ agentId, task, pri }: { agentId: string; task: string; pri: number }) =>
      orchestrationApi.createTask(agentId, task, pri),
    onSuccess: (_data, { agentId }) => {
      void qc.invalidateQueries({ queryKey: orchestrationKeys.tasks(agentId) });
      void qc.invalidateQueries({ queryKey: orchestrationKeys.queueDepth(agentId) });
      toast.success('Task queued');
      setPrompt('');
      onCreated();
    },
    onError: (err: unknown) => {
      toast.error(err instanceof Error ? err.message : 'Failed to create task');
    },
  });

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!prompt.trim()) return;

    let targetBridgeName: string | undefined;
    if (tool === 'Auto') {
      targetBridgeName = bridges[0]?.name;
    } else {
      targetBridgeName = TOOL_TO_BRIDGE[tool];
    }

    const agent = bridges.find((b) => b.name === targetBridgeName) ?? bridges[0];
    if (!agent) {
      toast.error('No bridge agent available');
      return;
    }

    mutation.mutate({ agentId: agent.id, task: prompt.trim(), pri: priority });
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div>
        <label className="text-[11px] uppercase tracking-wider text-sera-text-dim font-medium block mb-1.5">
          Prompt
        </label>
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="Describe the orchestration task…"
          rows={4}
          className="sera-input w-full resize-y text-sm"
        />
      </div>

      <div>
        <label
          htmlFor="task-tool"
          className="text-[11px] uppercase tracking-wider text-sera-text-dim font-medium block mb-1.5"
        >
          Bridge / Tool
        </label>
        <div className="relative">
          <select
            id="task-tool"
            value={tool}
            onChange={(e) => setTool(e.target.value)}
            className="sera-input w-full text-sm appearance-none pr-8"
          >
            {TOOL_OPTIONS.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
          <ChevronDown
            size={14}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-sera-text-dim pointer-events-none"
          />
        </div>
      </div>

      <div>
        <p className="text-[11px] uppercase tracking-wider text-sera-text-dim font-medium mb-1.5">
          Priority
        </p>
        <div
          className="flex items-center gap-1 border border-sera-border rounded-lg p-1"
          role="group"
          aria-label="Task priority"
        >
          {([1, 2, 3] as const).map((p) => (
            <button
              key={p}
              type="button"
              onClick={() => setPriority(p)}
              aria-pressed={priority === p}
              className={`flex-1 py-1.5 rounded-md text-xs font-medium transition-colors ${
                priority === p
                  ? 'bg-sera-accent-soft text-sera-accent'
                  : 'text-sera-text-muted hover:text-sera-text'
              }`}
            >
              {p === 1 ? 'High' : p === 2 ? 'Normal' : 'Low'}
            </button>
          ))}
        </div>
      </div>

      <Button
        type="submit"
        size="sm"
        className="w-full"
        disabled={mutation.isPending || !prompt.trim()}
      >
        {mutation.isPending ? 'Queuing…' : 'Queue Task'}
      </Button>
    </form>
  );
}

// ── Main Page ────────────────────────────────────────────────────────────────

function OrchestrationPageContent() {
  const qc = useQueryClient();
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const [toolFilter, setToolFilter] = useState('All');
  const [showCreate, setShowCreate] = useState(false);

  const { data: bridges, isLoading: bridgesLoading } = useQuery({
    queryKey: orchestrationKeys.bridges,
    queryFn: orchestrationApi.getBridgeAgents,
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  // Fetch tasks for each bridge agent
  const bridgeIds = useMemo(() => (bridges ?? []).map((b) => b.id), [bridges]);

  const taskQueries = bridgeIds.map((id) => ({
    id,
    query: useQuery({
      queryKey: orchestrationKeys.tasks(id, statusFilter === 'all' ? undefined : statusFilter),
      queryFn: () =>
        orchestrationApi.getAgentTasks(id, statusFilter === 'all' ? undefined : statusFilter),
      enabled: bridgeIds.length > 0,
      staleTime: 10_000,
      refetchInterval: 10_000,
    }),
  }));

  const allTasks = useMemo(() => {
    const results: Array<AgentTask & { agentName: string }> = [];
    for (const { id, query } of taskQueries) {
      if (query.data) {
        const bridge = (bridges ?? []).find((b) => b.id === id);
        const agentName = bridge ? (BRIDGE_NAMES[bridge.name] ?? bridge.name) : id;
        for (const t of query.data) {
          results.push({ ...t, agentName });
        }
      }
    }
    return results.sort(
      (a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()
    );
  }, [taskQueries, bridges]);

  const filteredTasks = useMemo(() => {
    if (toolFilter === 'All') return allTasks;
    const bridgeName = TOOL_TO_BRIDGE[toolFilter];
    const bridge = (bridges ?? []).find((b) => b.name === bridgeName);
    if (!bridge) return allTasks;
    return allTasks.filter((t) => t.agentInstanceId === bridge.id);
  }, [allTasks, toolFilter, bridges]);

  const isTasksLoading = taskQueries.some((q) => q.query.isLoading);

  function handleRefresh() {
    void qc.invalidateQueries({ queryKey: ['orchestration'] });
  }

  return (
    <main className="p-6 max-w-7xl mx-auto space-y-6">
      <header className="sera-page-header">
        <div>
          <h1 className="sera-page-title flex items-center gap-2">
            <GitBranch size={20} className="text-sera-accent" />
            Orchestration
          </h1>
          <p className="text-sm text-sera-text-muted mt-1">
            Bridge agents and task queue management
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={handleRefresh}>
            <RefreshCw size={13} />
            Refresh
          </Button>
          <Button size="sm" onClick={() => setShowCreate((v) => !v)}>
            <Plus size={13} />
            New Task
          </Button>
        </div>
      </header>

      {/* Bridge Status Cards */}
      <section aria-label="Bridge agents">
        <h2 className="sera-section-label mb-3">Bridge Status</h2>
        {bridgesLoading ? (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
            {[1, 2, 3, 4].map((i) => (
              <Skeleton key={i} className="h-24 rounded-xl" />
            ))}
          </div>
        ) : !bridges?.length ? (
          <p className="text-sm text-sera-text-muted py-4">
            No bridge agents found. Deploy an agent with a &ldquo;bridge&rdquo; template.
          </p>
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
            {bridges.map((b) => (
              <BridgeCard key={b.id} agent={b} />
            ))}
          </div>
        )}
      </section>

      <div className="flex gap-6 items-start">
        {/* Task List */}
        <section className="flex-1 min-w-0" aria-label="Task list">
          {/* Filters */}
          <div className="flex items-center gap-3 flex-wrap mb-4">
            {/* Tool filter */}
            <div className="relative">
              <select
                value={toolFilter}
                onChange={(e) => setToolFilter(e.target.value)}
                className="sera-input text-xs appearance-none pr-8"
                aria-label="Filter by bridge"
              >
                {TOOL_OPTIONS.map((t) => (
                  <option key={t} value={t}>
                    {t === 'All' ? 'All bridges' : t}
                  </option>
                ))}
              </select>
              <ChevronDown
                size={12}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-sera-text-dim pointer-events-none"
              />
            </div>

            {/* Status filter */}
            <div
              className="flex items-center gap-1 border border-sera-border rounded-lg p-1"
              role="group"
              aria-label="Filter by status"
            >
              {STATUS_OPTIONS.map((s) => (
                <button
                  key={s}
                  onClick={() => setStatusFilter(s)}
                  aria-pressed={statusFilter === s}
                  className={`px-2.5 py-1 rounded-md text-xs font-medium transition-colors ${
                    statusFilter === s
                      ? 'bg-sera-accent-soft text-sera-accent'
                      : 'text-sera-text-muted hover:text-sera-text'
                  }`}
                >
                  {s === 'all' ? 'All' : s.charAt(0).toUpperCase() + s.slice(1)}
                </button>
              ))}
            </div>

            <span className="text-xs text-sera-text-dim ml-auto">
              {filteredTasks.length} task{filteredTasks.length !== 1 ? 's' : ''}
            </span>
          </div>

          {/* Table */}
          <div className="sera-card-static overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm" aria-label="Orchestration tasks">
                <thead>
                  <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                    <th scope="col" className="text-left py-3 px-4">
                      ID
                    </th>
                    <th scope="col" className="text-left py-3 px-4">
                      Prompt
                    </th>
                    <th scope="col" className="text-left py-3 px-4">
                      Bridge
                    </th>
                    <th scope="col" className="text-left py-3 px-4">
                      Status
                    </th>
                    <th scope="col" className="text-left py-3 px-4">
                      Pri
                    </th>
                    <th scope="col" className="text-left py-3 px-4">
                      Created
                    </th>
                    <th scope="col" className="text-left py-3 px-4">
                      Duration
                    </th>
                    <th scope="col" className="py-3 px-4" aria-label="Expand" />
                  </tr>
                </thead>
                <tbody>
                  {isTasksLoading ? (
                    Array.from({ length: 5 }).map((_, i) => (
                      <tr key={i} className="border-b border-sera-border/50">
                        {Array.from({ length: 8 }).map((_, j) => (
                          <td key={j} className="py-3 px-4">
                            <Skeleton className="h-4 w-full" />
                          </td>
                        ))}
                      </tr>
                    ))
                  ) : filteredTasks.length === 0 ? (
                    <tr>
                      <td colSpan={8} className="py-12 text-center text-sera-text-dim text-sm">
                        {bridges?.length
                          ? 'No tasks match the current filters.'
                          : 'No bridge agents available.'}
                      </td>
                    </tr>
                  ) : (
                    filteredTasks.map((task) => (
                      <TaskRow key={task.id} task={task} agentName={task.agentName} />
                    ))
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </section>

        {/* Create Task Sidebar */}
        {showCreate && (
          <aside className="w-72 flex-shrink-0">
            <div className="sera-card-static p-4">
              <h2 className="text-sm font-semibold text-sera-text mb-4 flex items-center gap-2">
                <Plus size={14} className="text-sera-accent" />
                New Task
              </h2>
              <CreateTaskForm bridges={bridges ?? []} onCreated={() => setShowCreate(false)} />
            </div>
          </aside>
        )}
      </div>
    </main>
  );
}

export default function OrchestrationPage() {
  return (
    <ErrorBoundary fallbackMessage="The orchestration page encountered an error.">
      <OrchestrationPageContent />
    </ErrorBoundary>
  );
}
