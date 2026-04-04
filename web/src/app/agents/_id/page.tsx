import { useState, useMemo } from 'react';
import { useParams, useNavigate } from 'react-router';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { queryClient } from '@/lib/query-client';
import { Play, Square, RotateCcw, Bot, Trash2, Activity, Settings, Monitor } from 'lucide-react';
import { toast } from 'sonner';
import {
  useAgent,
  useStartAgent,
  useStopAgent,
  useRestartAgent,
  useDeleteAgent,
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
import { AgentDetailCommandsTab } from '@/components/AgentDetailCommandsTab';
import { AgentDetailHealthCheckTab } from '@/components/AgentDetailHealthCheckTab';
import { AgentDetailSystemPromptTab } from '@/components/AgentDetailSystemPromptTab';
import { AgentDetailTasksTab } from '@/components/AgentDetailTasksTab';
import { TemplateDiffBanner } from '@/components/TemplateDiffBanner';

type Tab = 'overview' | 'activity' | 'configuration' | 'observability';

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
    const overrides = agent.overrides ?? {};
    const obj = (key: string): Record<string, unknown> | undefined => {
      const v = overrides[key];
      return v && typeof v === 'object' && !Array.isArray(v)
        ? (v as Record<string, unknown>)
        : undefined;
    };
    const modelOv = obj('model');
    const resourcesOv = obj('resources');
    const permissionsOv = obj('permissions');
    const toolsOv = obj('tools');

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
              { id: 'overview', label: 'Overview', icon: Bot },
              { id: 'activity', label: 'Activity', icon: Activity },
              { id: 'configuration', label: 'Configuration', icon: Settings },
              { id: 'observability', label: 'Observability', icon: Monitor },
            ] as const
          ).map(({ id: tabId, label, icon: Icon }) => (
            <button
              key={tabId}
              onClick={() => setTab(tabId)}
              className={cn(
                'px-4 py-2 text-sm font-medium border-b-2 transition-colors flex items-center gap-2',
                tab === tabId
                  ? 'border-sera-accent text-sera-accent'
                  : 'border-transparent text-sera-text-muted hover:text-sera-text'
              )}
            >
              <Icon size={15} />
              {label}
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

          {tab === 'activity' && (
            <div className="p-6 space-y-6">
              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Activity size={15} /> Tasks
                </h3>
                <AgentDetailTasksTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Settings size={15} /> Delegations
                </h3>
                <AgentDetailDelegationsTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Monitor size={15} /> Inner Life
                </h3>
                <AgentDetailInnerLifeTab id={id} />
              </section>
            </div>
          )}

          {tab === 'configuration' && (
            <div className="p-6 space-y-6">
              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Settings size={15} /> Grants
                </h3>
                <AgentDetailGrantsTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Settings size={15} /> Tools
                </h3>
                <AgentDetailToolsTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Settings size={15} /> Budget
                </h3>
                <AgentDetailBudgetTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Settings size={15} /> Context
                </h3>
                <AgentDetailContextTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Settings size={15} /> System Prompt
                </h3>
                <AgentDetailSystemPromptTab id={id} />
              </section>
            </div>
          )}

          {tab === 'observability' && (
            <div className="p-6 space-y-6">
              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Monitor size={15} /> Logs
                </h3>
                <AgentDetailLogsTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Monitor size={15} /> Commands
                </h3>
                <AgentDetailCommandsTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Monitor size={15} /> Health Check
                </h3>
                <AgentDetailHealthCheckTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Monitor size={15} /> Memory
                </h3>
                <AgentDetailMemoryTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Monitor size={15} /> Core Memory
                </h3>
                <AgentDetailCoreMemoryTab id={id} />
              </section>

              <hr className="border-sera-border" />

              <section className="space-y-4">
                <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
                  <Monitor size={15} /> Schedules
                </h3>
                <AgentDetailSchedulesTab id={id} agentName={agent?.name} />
              </section>
            </div>
          )}
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
