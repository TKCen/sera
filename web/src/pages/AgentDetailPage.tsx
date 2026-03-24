import { useState } from 'react';
import { useParams, Link } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import { request } from '@/lib/api/client';
import {
  ArrowLeft,
  Play,
  Square,
  RotateCcw,
  Bot,
  Plus,
  Trash2,
  Shield,
  Check,
  X,
  AlertCircle,
} from 'lucide-react';
import { toast } from 'sonner';
import {
  useAgent,
  useAgentLogs,
  useAgentGrants,
  useCreateGrant,
  useRevokeGrant,
  usePermissionRequests,
  useDecidePermission,
  useStartAgent,
  useStopAgent,
  useRestartAgent,
} from '@/hooks/useAgents';
import { AgentStatusBadge } from '@/components/AgentStatusBadge';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
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
import { MemoryTab } from '@/components/AgentDetailMemoryTab';
import { SchedulesTab } from '@/components/AgentDetailSchedulesTab';
import { BudgetTab } from '@/components/AgentDetailBudgetTab';
import { DelegationsTab } from '@/components/AgentDetailDelegationsTab';

type Tab =
  | 'overview'
  | 'grants'
  | 'delegations'
  | 'logs'
  | 'memory'
  | 'schedules'
  | 'budget'
  | 'prompt';

export default function AgentDetailPage() {
  const { id = '' } = useParams<{ id: string }>();
  const [tab, setTab] = useState<Tab>('overview');
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
          </div>
        </div>

        {/* Tabs */}
        <div className="flex gap-0 mt-4">
          {(
            ['overview', 'grants', 'delegations', 'logs', 'memory', 'schedules', 'budget'] as const
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
        {tab === 'prompt' && <SystemPromptTab id={id} />}
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
  const { data: instance, isLoading } = useAgent(id);
  const [showRaw, setShowRaw] = useState(false);

  if (isLoading) return <TabLoading />;
  if (!instance) return <div className="p-6 text-sm text-sera-text-muted">Instance not found.</div>;

  const inst = instance as unknown as Record<string, unknown>;
  const overrides = (inst.overrides ?? {}) as Record<string, unknown>;
  const modelOv = overrides.model as Record<string, unknown> | undefined;
  const resourcesOv = overrides.resources as Record<string, unknown> | undefined;
  const resolvedCaps = (inst.resolved_capabilities ?? {}) as Record<string, unknown>;
  const permissions = overrides.permissions as Record<string, unknown> | undefined;
  const tools = overrides.tools as Record<string, unknown> | undefined;
  const skills = (overrides.skills as string[] | undefined) ?? [];

  return (
    <div className="p-6 space-y-4 max-w-3xl">
      {/* Identity */}
      <section className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Identity
        </h3>
        <div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
          <Field label="Name" value={inst.name as string} />
          <Field label="Display Name" value={(inst.display_name as string) || '—'} />
          <Field
            label="Template"
            value={(inst.template_ref as string) || (inst.template_name as string)}
          />
          <Field label="Circle" value={(inst.circle as string) || '—'} />
          <Field label="Lifecycle" value={(inst.lifecycle_mode as string) || 'persistent'} />
          <Field label="Workspace" value={(inst.workspace_path as string) || '—'} mono />
        </div>
      </section>

      {/* Model & Sandbox */}
      <section className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Model &amp; Sandbox
        </h3>
        <div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
          <Field label="Model" value={(modelOv?.name as string) || 'default'} mono />
          <Field label="Provider" value={(modelOv?.provider as string) || '—'} />
          <Field label="Temperature" value={String(modelOv?.temperature ?? '0.7')} />
          <Field
            label="Sandbox Boundary"
            value={
              (overrides.sandboxBoundary as string) || (inst.sandbox_boundary as string) || '—'
            }
          />
        </div>
      </section>

      {/* Resources */}
      <section className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Resources
        </h3>
        <div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
          <Field
            label="Tokens / Hour"
            value={
              resourcesOv?.maxLlmTokensPerHour
                ? (resourcesOv.maxLlmTokensPerHour as number).toLocaleString()
                : '—'
            }
          />
          <Field
            label="Tokens / Day"
            value={
              resourcesOv?.maxLlmTokensPerDay
                ? (resourcesOv.maxLlmTokensPerDay as number).toLocaleString()
                : '—'
            }
          />
        </div>
      </section>

      {/* Permissions & Tools */}
      {(permissions || tools || skills.length > 0) && (
        <section className="sera-card-static p-4">
          <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
            Permissions &amp; Tools
          </h3>
          <div className="space-y-2 text-xs">
            {permissions?.canExec !== undefined && (
              <Field
                label="Can Execute"
                value={String(permissions.canExec) === 'true' ? 'Yes' : 'No'}
              />
            )}
            {permissions?.canSpawnSubagents !== undefined && (
              <Field
                label="Can Spawn Subagents"
                value={String(permissions.canSpawnSubagents) === 'true' ? 'Yes' : 'No'}
              />
            )}
            {Array.isArray(tools?.allowed) && (tools.allowed as string[]).length > 0 && (
              <div>
                <span className="text-sera-text-muted">Tools Allowed: </span>
                <span className="text-sera-text font-mono">
                  {(tools.allowed as string[]).join(', ')}
                </span>
              </div>
            )}
            {Array.isArray(tools?.denied) && (tools.denied as string[]).length > 0 && (
              <div>
                <span className="text-sera-text-muted">Tools Denied: </span>
                <span className="text-sera-text font-mono">
                  {(tools.denied as string[]).join(', ')}
                </span>
              </div>
            )}
            {skills.length > 0 && (
              <div>
                <span className="text-sera-text-muted">Skills: </span>
                <span className="text-sera-text font-mono">{skills.join(', ')}</span>
              </div>
            )}
          </div>
        </section>
      )}

      {/* Resolved Capabilities */}
      {Object.keys(resolvedCaps).length > 0 && (
        <section className="sera-card-static p-4">
          <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
            Resolved Capabilities
          </h3>
          <div className="space-y-1 text-xs">
            {Object.entries(resolvedCaps).map(([key, value]) => (
              <div key={key} className="mb-2">
                <span className="text-sera-text-muted text-[11px] uppercase tracking-wider">{key}</span>
                {typeof value === 'object' && value !== null ? (
                  <div className="mt-1 ml-2 space-y-0.5">
                    {Object.entries(value as Record<string, unknown>).map(([k, v]) => (
                      <div key={k} className="flex items-start gap-2">
                        <span className="text-sera-text-dim min-w-[120px]">{k}:</span>
                        <span className="text-sera-text font-mono break-all">
                          {Array.isArray(v) ? v.join(', ') : typeof v === 'object' ? JSON.stringify(v, null, 2) : String(v)}
                        </span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <span className="text-sera-text font-mono ml-2">{String(value)}</span>
                )}
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Container / Runtime */}
      <section className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Runtime
        </h3>
        <div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
          <Field label="Status" value={(inst.status as string) || '—'} />
          <Field
            label="Container ID"
            value={(inst.container_id as string)?.slice(0, 12) || '—'}
            mono
          />
          <Field
            label="Created"
            value={inst.created_at ? new Date(inst.created_at as string).toLocaleString() : '—'}
          />
          <Field
            label="Updated"
            value={inst.updated_at ? new Date(inst.updated_at as string).toLocaleString() : '—'}
          />
          {typeof inst.last_heartbeat_at === 'string' && (
            <Field
              label="Last Heartbeat"
              value={new Date(inst.last_heartbeat_at as string).toLocaleString()}
            />
          )}
        </div>
      </section>

      {/* Raw JSON toggle */}
      <div>
        <button
          onClick={() => setShowRaw((p) => !p)}
          className="text-xs text-sera-text-dim hover:text-sera-text transition-colors"
        >
          {showRaw ? 'Hide' : 'Show'} raw JSON
        </button>
        {showRaw && (
          <pre className="sera-card-static p-4 mt-2 text-xs font-mono text-sera-text leading-relaxed overflow-x-auto whitespace-pre">
            {JSON.stringify(instance, null, 2)}
          </pre>
        )}
      </div>
    </div>
  );
}

function Field({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="text-sera-text-muted min-w-[120px]">{label}</span>
      <span className={cn('text-sera-text', mono && 'font-mono')}>{value}</span>
    </div>
  );
}

function GrantsTab({ id }: { id: string }) {
  const { data, isLoading } = useAgentGrants(id);
  const { data: pendingRequests } = usePermissionRequests(id);
  const createGrant = useCreateGrant();
  const revokeGrant = useRevokeGrant();
  const decidePermission = useDecidePermission();
  const [showAdd, setShowAdd] = useState(false);
  const [dimension, setDimension] = useState('filesystem');
  const [value, setValue] = useState('');
  const [grantType, setGrantType] = useState<'session' | 'persistent'>('persistent');

  if (isLoading) return <TabLoading />;

  const allGrants = [
    ...(data?.persistent ?? []).map((g) => ({ ...g, source: 'persistent' as const })),
    ...(data?.session ?? []).map((g) => ({ ...g, source: 'session' as const })),
  ];
  const activeGrants = allGrants.filter((g) => !g.revoked_at);
  const revokedGrants = allGrants.filter((g) => g.revoked_at);

  async function handleCreate() {
    if (!value.trim()) return;
    try {
      await createGrant.mutateAsync({
        id,
        params: { dimension, value: value.trim(), grantType },
      });
      setValue('');
      setShowAdd(false);
    } catch {
      // error handled by mutation
    }
  }

  return (
    <div className="p-6 space-y-4 max-w-3xl">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
          <Shield size={14} className="text-sera-accent" />
          Capability Grants
        </h3>
        <Button size="sm" variant="outline" onClick={() => setShowAdd((p) => !p)}>
          <Plus size={13} /> Add Grant
        </Button>
      </div>

      {/* Add Grant form */}
      {showAdd && (
        <div className="sera-card-static p-4 space-y-3 border-sera-accent/30">
          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">
                Dimension
              </label>
              <select
                value={dimension}
                onChange={(e) => setDimension(e.target.value)}
                className="sera-input text-xs"
              >
                <option value="filesystem">Filesystem</option>
                <option value="network">Network</option>
                <option value="exec.commands">Exec Commands</option>
              </select>
            </div>
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">Value</label>
              <Input
                value={value}
                onChange={(e) => setValue(e.target.value)}
                placeholder={
                  dimension === 'filesystem'
                    ? '/path/to/directory'
                    : dimension === 'network'
                      ? 'api.example.com'
                      : 'git'
                }
                className="text-xs"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">Type</label>
              <select
                value={grantType}
                onChange={(e) => setGrantType(e.target.value as 'session' | 'persistent')}
                className="sera-input text-xs"
              >
                <option value="persistent">Persistent</option>
                <option value="session">Session</option>
              </select>
            </div>
          </div>
          <div className="flex gap-2">
            <Button
              size="sm"
              onClick={() => {
                void handleCreate();
              }}
              disabled={!value.trim() || createGrant.isPending}
            >
              {createGrant.isPending ? 'Creating…' : 'Create Grant'}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setShowAdd(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {/* Pending permission requests */}
      {pendingRequests && pendingRequests.length > 0 && (
        <div className="space-y-2">
          <h4 className="text-xs font-semibold text-sera-warning flex items-center gap-1.5">
            <AlertCircle size={12} />
            {pendingRequests.length} Pending Request{pendingRequests.length > 1 ? 's' : ''}
          </h4>
          <div className="sera-card-static overflow-hidden border-sera-warning/30">
            {pendingRequests.map((req) => (
              <div
                key={req.requestId}
                className="flex items-center justify-between gap-3 px-3 py-2.5 border-b border-sera-border/50 last:border-0"
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 text-xs">
                    <Badge variant="accent">{req.dimension}</Badge>
                    <span className="font-mono text-sera-text truncate">{req.value}</span>
                  </div>
                  {req.reason && (
                    <p className="text-[10px] text-sera-text-muted mt-0.5 truncate">{req.reason}</p>
                  )}
                </div>
                <div className="flex items-center gap-1.5 shrink-0">
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 text-xs gap-1 text-green-400 border-green-400/30 hover:bg-green-400/10"
                    disabled={decidePermission.isPending}
                    onClick={() => {
                      void decidePermission.mutateAsync({
                        requestId: req.requestId,
                        agentId: id,
                        params: { decision: 'grant', grantType: 'session' },
                      });
                    }}
                  >
                    <Check size={11} /> Grant
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 text-xs gap-1 text-sera-error border-sera-error/30 hover:bg-sera-error/10"
                    disabled={decidePermission.isPending}
                    onClick={() => {
                      void decidePermission.mutateAsync({
                        requestId: req.requestId,
                        agentId: id,
                        params: { decision: 'deny' },
                      });
                    }}
                  >
                    <X size={11} /> Deny
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Active grants */}
      {activeGrants.length === 0 ? (
        <div className="sera-card-static p-8 text-center text-sm text-sera-text-muted">
          No active grants. Grants are created when agents request additional permissions or when
          operators add them manually.
        </div>
      ) : (
        <div className="sera-card-static overflow-hidden">
          <table className="w-full text-xs">
            <thead>
              <tr className="border-b border-sera-border text-[10px] uppercase tracking-wider text-sera-text-dim">
                <th className="text-left py-2.5 px-3">Dimension</th>
                <th className="text-left py-2.5 px-3">Value</th>
                <th className="text-left py-2.5 px-3">Type</th>
                <th className="text-left py-2.5 px-3">Granted</th>
                <th className="py-2.5 px-3" />
              </tr>
            </thead>
            <tbody>
              {activeGrants.map((g) => (
                <tr
                  key={g.id}
                  className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors"
                >
                  <td className="py-2.5 px-3">
                    <Badge variant="accent">{g.dimension}</Badge>
                  </td>
                  <td className="py-2.5 px-3 font-mono text-sera-text">{g.value}</td>
                  <td className="py-2.5 px-3">
                    <Badge variant={g.source === 'persistent' ? 'default' : 'warning'}>
                      {g.grant_type}
                    </Badge>
                  </td>
                  <td className="py-2.5 px-3 text-sera-text-muted">
                    {g.created_at ? new Date(g.created_at).toLocaleString() : '—'}
                  </td>
                  <td className="py-2.5 px-3 text-right">
                    <button
                      onClick={() => {
                        void revokeGrant.mutateAsync({ id, grantId: g.id });
                      }}
                      className="text-sera-text-dim hover:text-sera-error transition-colors p-1"
                      title="Revoke grant"
                    >
                      <Trash2 size={12} />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Revoked grants */}
      {revokedGrants.length > 0 && (
        <details className="text-xs">
          <summary className="text-sera-text-dim cursor-pointer hover:text-sera-text transition-colors py-2">
            {revokedGrants.length} revoked grant(s)
          </summary>
          <div className="sera-card-static overflow-hidden mt-2 opacity-60">
            <table className="w-full text-xs">
              <tbody>
                {revokedGrants.map((g) => (
                  <tr key={g.id} className="border-b border-sera-border/30">
                    <td className="py-2 px-3 text-sera-text-dim">{g.dimension}</td>
                    <td className="py-2 px-3 font-mono text-sera-text-dim line-through">
                      {g.value}
                    </td>
                    <td className="py-2 px-3 text-sera-text-dim">{g.grant_type}</td>
                    <td className="py-2 px-3 text-sera-text-dim">
                      Revoked {g.revoked_at ? new Date(g.revoked_at).toLocaleString() : ''}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </details>
      )}
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
