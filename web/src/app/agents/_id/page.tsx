import { useState, useMemo, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { queryClient } from '@/lib/query-client';
import { Play, Square, RotateCcw, Bot, Trash2, Check, AlertCircle } from 'lucide-react';
import { toast } from 'sonner';
import {
  useAgent,
  useStartAgent,
  useStopAgent,
  useRestartAgent,
  useDeleteAgent,
  useAgentSessions,
  useAgentHealthCheck,
  useAgentSystemPrompt,
  agentsKeys,
} from '@/hooks/useAgents';
import { AgentStatusBadge } from '@/components/AgentStatusBadge';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { CopyButton } from '@/components/CopyButton';
import { AgentForm, type AgentFormInitialValues } from '@/components/AgentForm';
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
import { Breadcrumbs } from '@/components/Breadcrumbs';
import { TabLoading } from '@/components/AgentDetailTabLoading';
import { AgentDetailManifestTab } from '@/components/AgentDetailManifestTab';
import { AgentDetailGrantsTab } from '@/components/AgentDetailGrantsTab';
import { AgentDetailToolsTab } from '@/components/AgentDetailToolsTab';
import { AgentDetailLogsTab } from '@/components/AgentDetailLogsTab';
import { AgentDetailMemoryTab } from '@/components/AgentDetailMemoryTab';
import { AgentDetailSchedulesTab } from '@/components/AgentDetailSchedulesTab';
import { AgentDetailBudgetTab } from '@/components/AgentDetailBudgetTab';
import { AgentDetailInnerLifeTab } from '@/components/AgentDetailInnerLifeTab';
import { AgentDetailDelegationsTab } from '@/components/AgentDetailDelegationsTab';
import { AgentDetailContextTab } from '@/components/AgentDetailContextTab';
import { AgentDetailCoreMemoryTab } from '@/components/AgentDetailCoreMemoryTab';
import { CommandLogTimeline } from '@/components/CommandLogTimeline';
import { AgentDetailTasksTab } from '@/components/AgentDetailTasksTab';
import { TemplateDiffBanner } from '@/components/TemplateDiffBanner';

type Tab =
  | 'overview'
  | 'tasks'
  | 'grants'
  | 'tools'
  | 'delegations'
  | 'logs'
  | 'commands'
  | 'memory'
  | 'core-memory'
  | 'schedules'
  | 'inner-life'
  | 'budget'
  | 'context'
  | 'prompt'
  | 'health';

export default function AgentDetailPage() {
  const { id = '' } = useParams<{ id: string }>();
  const [tab, setTab] = useState<Tab>('overview');
  const [confirmAction, setConfirmAction] = useState<'stop' | 'restart' | 'delete' | null>(null);
  const [isEditDialogOpen, setIsEditDialogOpen] = useState(false);

  const { data: agent, isLoading } = useAgent(id);
  const navigate = useNavigate();
  const startAgent = useStartAgent();
  const stopAgent = useStopAgent();
  const restartAgent = useRestartAgent();
  const deleteAgent = useDeleteAgent();

  const initialValues: AgentFormInitialValues | undefined = useMemo(() => {
    if (!agent) return undefined;
    const overrides = (agent.overrides ?? {}) as Record<string, unknown>;
    const modelOv = overrides.model as Record<string, unknown> | undefined;
    const resourcesOv = overrides.resources as Record<string, unknown> | undefined;
    const permissionsOv = overrides.permissions as Record<string, unknown> | undefined;
    const toolsOv = overrides.tools as Record<string, unknown> | undefined;

    return {
      templateRef: agent.template_ref,
      name: agent.name,
      displayName: agent.display_name ?? '',
      circle: agent.circle ?? '',
      lifecycleMode: (agent.lifecycle_mode as 'persistent' | 'ephemeral') ?? 'persistent',
      modelName: (modelOv?.name as string) ?? '',
      modelProvider: (modelOv?.provider as string) ?? '',
      temperature: (modelOv?.temperature as number) ?? 0.7,
      sandboxBoundary: (overrides.sandboxBoundary as string) ?? 'tier-2',
      tokensPerHour: (resourcesOv?.maxLlmTokensPerHour as number) ?? 100000,
      tokensPerDay: (resourcesOv?.maxLlmTokensPerDay as number) ?? 1000000,
      canExec: (permissionsOv?.canExec as boolean) ?? false,
      canSpawnSubagents: (permissionsOv?.canSpawnSubagents as boolean) ?? false,
      toolsAllowed: Array.isArray(toolsOv?.allowed) ? (toolsOv.allowed as string[]) : [],
      toolsDenied: Array.isArray(toolsOv?.denied) ? (toolsOv.denied as string[]) : [],
      skills: Array.isArray(overrides.skills) ? (overrides.skills as string[]) : [],
    };
  }, [agent]);

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
        <Breadcrumbs items={[{ label: 'Agents', href: '/agents' }, { label: displayName }]} />

        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="h-10 w-10 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
              <Bot size={18} className="text-sera-accent" />
            </div>
            <div>
              <h1 className="text-xl font-semibold text-sera-text leading-tight">{displayName}</h1>
              <div className="flex items-center gap-2 mt-1">
                <div className="flex items-center gap-1">
                  <span className="text-xs text-sera-text-dim">{id}</span>
                  <CopyButton value={id} />
                </div>
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
              disabled={startAgent.isPending || agent?.status === 'running'}
            >
              <Play size={13} /> Start
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => setConfirmAction('stop')}
              disabled={stopAgent.isPending || agent?.status === 'stopped'}
            >
              <Square size={13} /> Stop
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => setConfirmAction('restart')}
              disabled={restartAgent.isPending}
            >
              <RotateCcw size={13} /> Restart
            </Button>
            <Button size="sm" variant="outline" onClick={() => setIsEditDialogOpen(true)}>
              Edit
            </Button>
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
              'tasks',
              'grants',
              'tools',
              'delegations',
              'logs',
              'commands',
              'memory',
              'core-memory',
              'schedules',
              'inner-life',
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

      {/* Template diff banner */}
      {id && <TemplateDiffBanner agentId={id} />}

      {/* Tab content */}
      <div className="flex-1 overflow-y-auto">
        <ErrorBoundary
          onReset={() => {
            void queryClient.invalidateQueries({ queryKey: agentsKeys.detail(id) });
          }}
          fallbackMessage="The agent detail tab content failed to load."
        >
          {tab === 'overview' && <AgentDetailManifestTab id={id} />}
          {tab === 'tasks' && <AgentDetailTasksTab id={id} />}
          {tab === 'grants' && <AgentDetailGrantsTab id={id} />}
          {tab === 'tools' && <AgentDetailToolsTab id={id} />}
          {tab === 'delegations' && <AgentDetailDelegationsTab id={id} />}
          {tab === 'logs' && <AgentDetailLogsTab id={id} />}
          {tab === 'commands' && <CommandsTab id={id} />}
          {tab === 'memory' && <AgentDetailMemoryTab id={id} />}
          {tab === 'core-memory' && <AgentDetailCoreMemoryTab id={id} />}
          {tab === 'schedules' && <AgentDetailSchedulesTab id={id} agentName={agent?.name} />}
          {tab === 'inner-life' && <AgentDetailInnerLifeTab id={id} />}
          {tab === 'budget' && <AgentDetailBudgetTab id={id} />}
          {tab === 'context' && <AgentDetailContextTab id={id} />}
          {tab === 'prompt' && <SystemPromptTab id={id} />}
          {tab === 'health' && <HealthCheckTab id={id} />}
        </ErrorBoundary>
      </div>

      {/* Edit agent dialog */}
      <Dialog open={isEditDialogOpen} onOpenChange={setIsEditDialogOpen}>
        <DialogContent className="max-w-2xl max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Edit Agent</DialogTitle>
            <DialogDescription>Modify agent configuration and overrides.</DialogDescription>
          </DialogHeader>
          <div className="mt-4">
            <AgentForm
              mode="edit"
              instanceId={id}
              initialValues={initialValues}
              onSuccess={() => setIsEditDialogOpen(false)}
              onCancel={() => setIsEditDialogOpen(false)}
            />
          </div>
        </DialogContent>
      </Dialog>

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

function CommandsTab({ id }: { id: string }) {
  const { data: sessions, isLoading } = useAgentSessions(id);

  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);

  if (isLoading) return <TabLoading />;

  const sortedSessions = useMemo(
    () =>
      [...(sessions ?? [])].sort(
        (a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
      ),
    [sessions]
  );

  // Auto-select most recent session
  useEffect(() => {
    if (!selectedSessionId && sortedSessions.length > 0) {
      setSelectedSessionId(sortedSessions[0]!.id);
    }
  }, [sortedSessions, selectedSessionId]);

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex-shrink-0 border-b border-sera-border bg-sera-surface-bright/20 p-3">
        <div className="flex items-center gap-3">
          <label className="text-[10px] font-bold text-sera-text-muted uppercase tracking-wider flex items-center gap-1.5">
            Session Context:
          </label>
          <select
            value={selectedSessionId ?? ''}
            onChange={(e) => setSelectedSessionId(e.target.value)}
            className="text-xs bg-sera-surface border border-sera-border rounded-md px-3 py-1.5 min-w-[240px] font-medium outline-none focus:ring-1 focus:ring-sera-accent"
          >
            {sortedSessions.map((s) => (
              <option key={s.id} value={s.id}>
                {s.title || 'Untitled Session'} — {new Date(s.updatedAt).toLocaleDateString()}
              </option>
            ))}
            {sortedSessions.length === 0 && <option value="">No sessions found</option>}
          </select>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {selectedSessionId ? (
          <CommandLogTimeline agentId={id} sessionId={selectedSessionId} />
        ) : (
          <div className="flex flex-col items-center justify-center p-12 text-sera-text-muted opacity-50">
            <Bot size={40} className="mb-4 text-sera-accent-soft" />
            <p className="text-sm font-medium">Select a session to view its command log</p>
          </div>
        )}
      </div>
    </div>
  );
}

function HealthCheckTab({ id }: { id: string }) {
  const { data, isLoading, refetch, isFetching } = useAgentHealthCheck(id);

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
  const { data, isLoading } = useAgentSystemPrompt(id);

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
