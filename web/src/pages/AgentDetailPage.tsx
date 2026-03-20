import { useState } from 'react';
import { useParams, Link } from 'react-router';
import {
  ArrowLeft,
  Play,
  Square,
  RotateCcw,
  Bot,
  ExternalLink,
  Calendar,
  Clock,
  Edit2,
  Check,
  X,
  RotateCw,
} from 'lucide-react';
import { toast } from 'sonner';
import {
  useAgent,
  useAgentManifestRaw,
  useAgentLogs,
  useAgentSchedules,
  useAgentMemory,
  useStartAgent,
  useStopAgent,
  useRestartAgent,
} from '@/hooks/useAgents';
import { useAgentBudget, usePatchAgentBudget, useResetAgentBudget } from '@/hooks/useUsage';
import { AgentStatusBadge } from '@/components/AgentStatusBadge';
import { BudgetBar } from '@/components/BudgetBar';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';
import { cn } from '@/lib/utils';

type Tab = 'manifest' | 'logs' | 'memory' | 'schedules' | 'budget';

export default function AgentDetailPage() {
  const { id = '' } = useParams<{ id: string }>();
  const [tab, setTab] = useState<Tab>('manifest');
  const [confirmAction, setConfirmAction] = useState<'stop' | 'restart' | null>(null);

  const { data: agent, isLoading } = useAgent(id);
  const startAgent = useStartAgent();
  const stopAgent = useStopAgent();
  const restartAgent = useRestartAgent();

  async function handleLifecycle(action: 'start' | 'stop' | 'restart') {
    try {
      if (action === 'start') {
        await startAgent.mutateAsync(id);
        toast.success('Agent starting…');
      } else if (action === 'stop') {
        await stopAgent.mutateAsync(id);
        toast.success('Agent stopping…');
      } else {
        await restartAgent.mutateAsync(id);
        toast.success('Agent restarting…');
      }
    } catch (err) {
      toast.error(err instanceof Error ? err.message : `Failed to ${action}`);
    } finally {
      setConfirmAction(null);
    }
  }

  if (isLoading) {
    return (
      <div className="p-6 space-y-4">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-32 rounded-xl" />
      </div>
    );
  }

  const displayName = agent?.displayName ?? id;

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-6 pt-6 pb-4 border-b border-sera-border flex-shrink-0">
        <Link
          to="/agents"
          className="inline-flex items-center gap-1.5 text-xs text-sera-text-muted hover:text-sera-text mb-4 transition-colors"
        >
          <ArrowLeft size={12} /> Agents
        </Link>

        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="h-10 w-10 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
              <Bot size={18} className="text-sera-accent" />
            </div>
            <div>
              <h1 className="text-xl font-semibold text-sera-text leading-tight">{displayName}</h1>
              <div className="flex items-center gap-2 mt-1">
                <span className="text-xs text-sera-text-dim">{id}</span>
                {agent?.circle && <Badge variant="default">{agent.circle}</Badge>}
                {agent?.model?.name && <Badge variant="accent">{agent.model.name}</Badge>}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2 flex-shrink-0">
            <AgentStatusBadge agentId={id} staticStatus={agent?.status} />
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                void handleLifecycle('start');
              }}
              disabled={startAgent.isPending}
            >
              <Play size={13} /> Start
            </Button>
            <Button size="sm" variant="outline" onClick={() => setConfirmAction('stop')}>
              <Square size={13} /> Stop
            </Button>
            <Button size="sm" variant="outline" onClick={() => setConfirmAction('restart')}>
              <RotateCcw size={13} /> Restart
            </Button>
            <Button size="sm" asChild variant="ghost">
              <Link to={`/agents/${id}/edit`}>Edit</Link>
            </Button>
          </div>
        </div>

        {/* Tabs */}
        <div className="flex gap-0 mt-4">
          {(['manifest', 'logs', 'memory', 'schedules', 'budget'] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={cn(
                'px-4 py-2 text-sm font-medium border-b-2 transition-colors',
                tab === t
                  ? 'border-sera-accent text-sera-accent'
                  : 'border-transparent text-sera-text-muted hover:text-sera-text'
              )}
            >
              {t.charAt(0).toUpperCase() + t.slice(1)}
            </button>
          ))}
        </div>
      </div>

      {/* Tab content */}
      <div className="flex-1 overflow-y-auto">
        {tab === 'manifest' && <ManifestTab id={id} />}
        {tab === 'logs' && <LogsTab id={id} />}
        {tab === 'memory' && <MemoryTab id={id} />}
        {tab === 'schedules' && <SchedulesTab id={id} />}
        {tab === 'budget' && <BudgetTab id={id} />}
      </div>

      {/* Confirmation dialog */}
      <Dialog
        open={confirmAction !== null}
        onOpenChange={(o: boolean) => !o && setConfirmAction(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{confirmAction === 'stop' ? 'Stop agent' : 'Restart agent'}</DialogTitle>
            <DialogDescription>
              {confirmAction === 'stop'
                ? `This will stop ${displayName}. Any running tasks will be interrupted.`
                : `This will restart ${displayName}. The agent will briefly go offline.`}
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
              variant={confirmAction === 'stop' ? 'danger' : 'outline'}
              onClick={() => {
                void handleLifecycle(confirmAction!);
              }}
            >
              {confirmAction === 'stop' ? 'Stop' : 'Restart'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function ManifestTab({ id }: { id: string }) {
  const { data: raw, isLoading } = useAgentManifestRaw(id);

  if (isLoading) return <TabLoading />;

  return (
    <div className="p-6">
      <pre className="sera-card-static p-4 text-xs font-mono text-sera-text leading-relaxed overflow-x-auto whitespace-pre">
        {raw ?? 'No manifest found.'}
      </pre>
    </div>
  );
}

function LogsTab({ id }: { id: string }) {
  const { data: logs, isLoading, refetch } = useAgentLogs(id);

  return (
    <div className="p-6 flex flex-col gap-3 h-full">
      <div className="flex items-center justify-between">
        <span className="text-xs text-sera-text-muted">Auto-refreshes every 3s</span>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            void refetch();
          }}
        >
          Refresh
        </Button>
      </div>
      {isLoading ? (
        <TabLoading />
      ) : (
        <pre className="flex-1 sera-card-static p-4 text-xs font-mono text-sera-text leading-relaxed overflow-auto whitespace-pre">
          {logs || 'No logs.'}
        </pre>
      )}
    </div>
  );
}

function MemoryTab({ id }: { id: string }) {
  const [scope, setScope] = useState<string>('');
  const { data: blocks, isLoading } = useAgentMemory(id, scope || undefined);

  return (
    <div className="p-6 space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex gap-1">
          {(['', 'personal', 'circle', 'global'] as const).map((s) => (
            <button
              key={s}
              onClick={() => setScope(s)}
              className={cn(
                'px-3 py-1.5 rounded-md text-xs font-medium transition-colors',
                scope === s
                  ? 'bg-sera-accent-soft text-sera-accent'
                  : 'text-sera-text-muted hover:bg-sera-surface-hover'
              )}
            >
              {s === '' ? 'All' : s.charAt(0).toUpperCase() + s.slice(1)}
            </button>
          ))}
        </div>
        <Link
          to={`/agents/${id}/memory-graph`}
          className="flex items-center gap-1 text-xs text-sera-accent hover:underline"
        >
          <ExternalLink size={11} /> View graph
        </Link>
      </div>

      {isLoading ? (
        <TabLoading />
      ) : !blocks?.length ? (
        <p className="text-sm text-sera-text-muted text-center py-8">No memory blocks.</p>
      ) : (
        <div className="space-y-2">
          {blocks.map((block) => (
            <Link
              key={block.id}
              to={`/memory/${block.id}`}
              className="sera-card flex items-start gap-3 p-3 block"
            >
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-0.5">
                  <span className="text-sm font-medium text-sera-text truncate">{block.title}</span>
                  <Badge variant="accent">{block.type}</Badge>
                  <Badge variant="default">{block.scope}</Badge>
                </div>
                {block.tags && block.tags.length > 0 && (
                  <div className="flex gap-1 flex-wrap mt-1">
                    {block.tags.map((tag) => (
                      <span
                        key={tag}
                        className="text-[10px] text-sera-text-dim bg-sera-surface-active px-1.5 py-0.5 rounded"
                      >
                        {tag}
                      </span>
                    ))}
                  </div>
                )}
              </div>
              {block.updatedAt && (
                <span className="text-[10px] text-sera-text-dim flex-shrink-0 flex items-center gap-1 mt-0.5">
                  <Clock size={9} /> {new Date(block.updatedAt).toLocaleDateString()}
                </span>
              )}
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

function SchedulesTab({ id }: { id: string }) {
  const { data: schedules, isLoading } = useAgentSchedules(id);

  if (isLoading) return <TabLoading />;

  if (!schedules?.length) {
    return (
      <div className="p-6">
        <p className="text-sm text-sera-text-muted text-center py-8">No schedules configured.</p>
      </div>
    );
  }

  return (
    <div className="p-6 space-y-2">
      {schedules.map((sched) => (
        <div key={sched.id} className="sera-card-static p-4 flex items-center gap-4">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <span className="font-mono text-sm text-sera-accent">{sched.cron}</span>
              {sched.description && (
                <span className="text-sm text-sera-text">{sched.description}</span>
              )}
              <Badge variant={sched.enabled ? 'success' : 'default'}>
                {sched.enabled ? 'enabled' : 'disabled'}
              </Badge>
            </div>
            <div className="flex items-center gap-4 text-xs text-sera-text-muted">
              {sched.lastRunAt && (
                <span className="flex items-center gap-1">
                  <Clock size={10} /> Last: {new Date(sched.lastRunAt).toLocaleString()}
                  {sched.lastRunStatus && (
                    <Badge variant={sched.lastRunStatus === 'success' ? 'success' : 'error'}>
                      {sched.lastRunStatus}
                    </Badge>
                  )}
                </span>
              )}
              {sched.nextRunAt && (
                <span className="flex items-center gap-1">
                  <Calendar size={10} /> Next: {new Date(sched.nextRunAt).toLocaleString()}
                </span>
              )}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

function BudgetTab({ id }: { id: string }) {
  const { data: budget, isLoading, refetch } = useAgentBudget(id);
  const patchBudget = usePatchAgentBudget(id);
  const resetBudget = useResetAgentBudget(id);

  const [editingHour, setEditingHour] = useState(false);
  const [editingDay, setEditingDay] = useState(false);
  const [hourVal, setHourVal] = useState('');
  const [dayVal, setDayVal] = useState('');

  const startEditHour = () => {
    setHourVal(String(budget?.maxLlmTokensPerHour ?? ''));
    setEditingHour(true);
  };

  const startEditDay = () => {
    setDayVal(String(budget?.maxLlmTokensPerDay ?? ''));
    setEditingDay(true);
  };

  const saveHour = async () => {
    const val = hourVal === '' ? null : Number(hourVal);
    try {
      await patchBudget.mutateAsync({ maxLlmTokensPerHour: val });
      toast.success('Hourly limit updated');
    } catch {
      toast.error('Failed to update hourly limit');
    }
    setEditingHour(false);
  };

  const saveDay = async () => {
    const val = dayVal === '' ? null : Number(dayVal);
    try {
      await patchBudget.mutateAsync({ maxLlmTokensPerDay: val });
      toast.success('Daily limit updated');
    } catch {
      toast.error('Failed to update daily limit');
    }
    setEditingDay(false);
  };

  const handleReset = async () => {
    try {
      await resetBudget.mutateAsync();
      toast.success('Budget counters reset');
    } catch {
      toast.error('Failed to reset budget');
    }
  };

  if (isLoading) return <TabLoading />;

  const hourPct = budget?.maxLlmTokensPerHour
    ? (budget.currentHourTokens / budget.maxLlmTokensPerHour) * 100
    : 0;
  const dayPct = budget?.maxLlmTokensPerDay
    ? (budget.currentDayTokens / budget.maxLlmTokensPerDay) * 100
    : 0;
  const exceeded = hourPct >= 100 || dayPct >= 100;

  return (
    <div className="p-6 space-y-6 max-w-xl">
      {exceeded && (
        <div className="px-4 py-3 rounded-lg bg-sera-error/10 border border-sera-error/30 text-sera-error text-sm font-medium">
          Budget exceeded — agent requests are being rejected until the period resets or the budget
          is adjusted.
        </div>
      )}

      <div className="sera-card-static p-5 space-y-5">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-sera-text">Token Budget</h3>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              void handleReset();
            }}
            disabled={resetBudget.isPending}
          >
            <RotateCw size={13} />
            Reset Counters
          </Button>
        </div>

        {/* Hourly */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-sera-text-muted uppercase tracking-wider">
              Hourly Limit
            </span>
            {!editingHour ? (
              <button
                onClick={startEditHour}
                className="flex items-center gap-1 text-xs text-sera-text-dim hover:text-sera-text transition-colors"
              >
                <Edit2 size={11} />
                {budget?.maxLlmTokensPerHour !== undefined
                  ? budget.maxLlmTokensPerHour.toLocaleString()
                  : 'Unlimited'}
              </button>
            ) : (
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={hourVal}
                  onChange={(e) => setHourVal(e.target.value)}
                  placeholder="unlimited"
                  className="sera-input text-xs w-32"
                  autoFocus
                />
                <button
                  onClick={() => {
                    void saveHour();
                  }}
                  className="text-sera-success hover:opacity-80"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setEditingHour(false)}
                  className="text-sera-text-dim hover:text-sera-text"
                >
                  <X size={14} />
                </button>
              </div>
            )}
          </div>
          <BudgetBar
            label="This hour"
            current={budget?.currentHourTokens ?? 0}
            limit={budget?.maxLlmTokensPerHour}
          />
        </div>

        {/* Daily */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-sera-text-muted uppercase tracking-wider">
              Daily Limit
            </span>
            {!editingDay ? (
              <button
                onClick={startEditDay}
                className="flex items-center gap-1 text-xs text-sera-text-dim hover:text-sera-text transition-colors"
              >
                <Edit2 size={11} />
                {budget?.maxLlmTokensPerDay !== undefined
                  ? budget.maxLlmTokensPerDay.toLocaleString()
                  : 'Unlimited'}
              </button>
            ) : (
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={dayVal}
                  onChange={(e) => setDayVal(e.target.value)}
                  placeholder="unlimited"
                  className="sera-input text-xs w-32"
                  autoFocus
                />
                <button
                  onClick={() => {
                    void saveDay();
                  }}
                  className="text-sera-success hover:opacity-80"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setEditingDay(false)}
                  className="text-sera-text-dim hover:text-sera-text"
                >
                  <X size={14} />
                </button>
              </div>
            )}
          </div>
          <BudgetBar
            label="Today"
            current={budget?.currentDayTokens ?? 0}
            limit={budget?.maxLlmTokensPerDay}
          />
        </div>
      </div>

      <button
        onClick={() => {
          void refetch();
        }}
        className="text-xs text-sera-text-dim hover:text-sera-text transition-colors"
      >
        Refresh usage counters
      </button>
    </div>
  );
}

function TabLoading() {
  return (
    <div className="p-6 space-y-3">
      <Skeleton className="h-6 w-full" />
      <Skeleton className="h-6 w-3/4" />
      <Skeleton className="h-6 w-1/2" />
    </div>
  );
}
