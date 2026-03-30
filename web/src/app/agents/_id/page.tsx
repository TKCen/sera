import { useState } from 'react';
import { useParams, Link, useNavigate } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import { request } from '@/lib/api/client';
import { ArrowLeft, Play, Square, RotateCcw, Bot, Trash2, Check, AlertCircle } from 'lucide-react';
import { toast } from 'sonner';
import {
  useAgent,
  useStartAgent,
  useStopAgent,
  useRestartAgent,
  useDeleteAgent,
} from '@/hooks/useAgents';
import { AgentStatusBadge } from '@/components/AgentStatusBadge';
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
import { TabLoading } from '@/components/AgentDetailTabLoading';
import { AgentDetailManifestTab as ManifestTab } from '@/components/AgentDetailManifestTab';
import { AgentDetailGrantsTab as GrantsTab } from '@/components/AgentDetailGrantsTab';
import { AgentDetailLogsTab as LogsTab } from '@/components/AgentDetailLogsTab';
import { MemoryTab } from '@/components/AgentDetailMemoryTab';
import { SchedulesTab } from '@/components/AgentDetailSchedulesTab';
import { BudgetTab } from '@/components/AgentDetailBudgetTab';
import { DelegationsTab } from '@/components/AgentDetailDelegationsTab';
import { ContextTab } from '@/components/AgentDetailContextTab';

type Tab =
  | 'overview'
  | 'grants'
  | 'delegations'
  | 'logs'
  | 'memory'
  | 'schedules'
  | 'budget'
  | 'context'
  | 'prompt'
  | 'health';

export default function AgentDetailPage() {
  const { id = '' } = useParams<{ id: string }>();
  const [tab, setTab] = useState<Tab>('overview');
  const [confirmAction, setConfirmAction] = useState<'stop' | 'restart' | 'delete' | null>(null);

  const { data: agent, isLoading } = useAgent(id);
  const navigate = useNavigate();
  const startAgent = useStartAgent();
  const stopAgent = useStopAgent();
  const restartAgent = useRestartAgent();
  const deleteAgent = useDeleteAgent();

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

  const displayName = agent?.display_name ?? agent?.name ?? id;

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
                {agent?.template_ref && <Badge variant="default">{agent.template_ref}</Badge>}
                {agent?.circle && <Badge variant="default">{agent.circle}</Badge>}
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
            <Link
              to={`/agents/${id}/edit`}
              className="inline-flex items-center gap-1.5 h-8 px-3 text-xs font-medium rounded-md border border-sera-border hover:bg-sera-surface transition-colors text-sera-text"
            >
              Edit
            </Link>
            <Button size="sm" variant="danger" onClick={() => setConfirmAction('delete')}>
              <Trash2 size={13} /> Delete
            </Button>
          </div>
        </div>

        {/* Tabs */}
        <div className="flex gap-0 mt-4">
          {(
            [
              'overview',
              'grants',
              'delegations',
              'logs',
              'memory',
              'schedules',
              'budget',
              'context',
              'health',
            ] as const
          ).map((t) => (
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
        {tab === 'overview' && <ManifestTab id={id} />}
        {tab === 'grants' && <GrantsTab id={id} />}
        {tab === 'delegations' && <DelegationsTab id={id} />}
        {tab === 'logs' && <LogsTab id={id} />}
        {tab === 'memory' && <MemoryTab id={id} />}
        {tab === 'schedules' && <SchedulesTab id={id} />}
        {tab === 'budget' && <BudgetTab id={id} />}
        {tab === 'context' && <ContextTab id={id} />}
        {tab === 'prompt' && <SystemPromptTab id={id} />}
        {tab === 'health' && <HealthCheckTab id={id} />}
      </div>

      {/* Confirmation dialog */}
      <Dialog
        open={confirmAction !== null}
        onOpenChange={(o: boolean) => !o && setConfirmAction(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {confirmAction === 'stop'
                ? 'Stop agent'
                : confirmAction === 'delete'
                  ? 'Delete agent'
                  : 'Restart agent'}
            </DialogTitle>
            <DialogDescription>
              {confirmAction === 'stop'
                ? `This will stop ${displayName}. Any running tasks will be interrupted.`
                : confirmAction === 'delete'
                  ? `This will permanently delete ${displayName}. This cannot be undone.`
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
              variant={
                confirmAction === 'stop' || confirmAction === 'delete' ? 'danger' : 'outline'
              }
              onClick={() => {
                if (confirmAction === 'delete') {
                  void deleteAgent.mutateAsync(id).then(() => {
                    toast.success(`Deleted ${displayName}`);
                    void navigate('/agents');
                  });
                } else {
                  void handleLifecycle(confirmAction!);
                }
              }}
            >
              {confirmAction === 'stop'
                ? 'Stop'
                : confirmAction === 'delete'
                  ? 'Delete'
                  : 'Restart'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

interface HealthCheckResult {
  agentId: string;
  agentName?: string;
  overallStatus: string;
  checks: Record<string, { ok: boolean; detail?: string }>;
}

function HealthCheckTab({ id }: { id: string }) {
  const { data, isLoading, refetch, isFetching } = useQuery({
    queryKey: ['agent-health-check', id],
    queryFn: () => request<HealthCheckResult>(`/agents/${encodeURIComponent(id)}/health-check`),
    enabled: id.length > 0,
  });

  if (isLoading) return <TabLoading />;

  const overallColor =
    data?.overallStatus === 'healthy'
      ? 'text-sera-success'
      : data?.overallStatus === 'degraded'
        ? 'text-yellow-400'
        : 'text-sera-error';

  return (
    <div className="p-6 max-w-2xl space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-semibold text-sera-text">Health Check</h3>
          <p className="text-xs text-sera-text-muted mt-0.5">
            Diagnostic checks for this agent instance
          </p>
        </div>
        <Button size="sm" variant="outline" onClick={() => void refetch()} disabled={isFetching}>
          <RotateCcw size={12} className={isFetching ? 'animate-spin' : ''} /> Re-check
        </Button>
      </div>

      {data && (
        <>
          <div className={cn('text-sm font-semibold', overallColor)}>
            Status: {data.overallStatus}
          </div>

          <div className="sera-card-static divide-y divide-sera-border/50">
            {Object.entries(data.checks).map(([name, check]) => (
              <div key={name} className="flex items-center gap-3 px-4 py-3">
                {check.ok ? (
                  <Check size={14} className="text-sera-success flex-shrink-0" />
                ) : (
                  <AlertCircle size={14} className="text-sera-error flex-shrink-0" />
                )}
                <span className="text-sm text-sera-text font-medium flex-1">{name}</span>
                {check.detail && (
                  <span className="text-xs text-sera-text-muted">{check.detail}</span>
                )}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function SystemPromptTab({ id }: { id: string }) {
  const { data, isLoading } = useQuery({
    queryKey: ['agent-system-prompt', id],
    queryFn: () => request<{ prompt: string }>(`/agents/${encodeURIComponent(id)}/system-prompt`),
    enabled: id.length > 0,
  });

  if (isLoading) return <TabLoading />;

  return (
    <div className="p-6 max-w-4xl">
      <h3 className="text-sm font-semibold text-sera-text mb-3">Resolved System Prompt</h3>
      <p className="text-xs text-sera-text-muted mb-4">
        This is the full system prompt sent to the LLM on each request, built from the agent&apos;s
        template identity, tools, and configuration.
      </p>
      <pre className="sera-card-static p-4 text-xs font-mono text-sera-text leading-relaxed overflow-auto whitespace-pre-wrap max-h-[70vh]">
        {data?.prompt || 'Unable to generate system prompt.'}
      </pre>
    </div>
  );
}
