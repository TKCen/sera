import { useState, useCallback } from 'react';
import {
  Play,
  Pencil,
  Trash2,
  Check,
  X,
  Info,
  ChevronDown,
  ChevronRight,
  Plus,
} from 'lucide-react';
import { toast } from 'sonner';
import { formatDistanceToNow } from '@/lib/utils';
import {
  useSchedules,
  useUpdateSchedule,
  useDeleteSchedule,
  useTriggerSchedule,
  useCreateSchedule,
} from '@/hooks/useSchedules';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
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
import { Tooltip } from '@/components/ui/tooltip';
import type { Schedule } from '@/lib/api/types';
import { useAgents } from '@/hooks/useAgents';

function statusBadge(status: Schedule['status']) {
  return <Badge variant={status === 'active' ? 'success' : 'default'}>{status}</Badge>;
}

function lastRunBadge(s?: Schedule['lastRunStatus']) {
  if (!s) return null;
  const variant = s === 'success' ? 'success' : s === 'error' ? 'error' : 'warning';
  return <Badge variant={variant}>{s}</Badge>;
}

function ScheduleRow({ sched }: { sched: Schedule }) {
  const [editing, setEditing] = useState(false);
  const [expr, setExpr] = useState(sched.expression);
  const [prompt, setPrompt] = useState(sched.taskPrompt ?? '');
  const [expanded, setExpanded] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [confirmTrigger, setConfirmTrigger] = useState(false);

  const updateSchedule = useUpdateSchedule();
  const deleteSchedule = useDeleteSchedule();
  const triggerSchedule = useTriggerSchedule();

  const isManifest = sched.source === 'manifest';

  const handleSave = useCallback(async () => {
    try {
      await updateSchedule.mutateAsync({
        id: sched.id,
        data: { expression: expr, taskPrompt: prompt },
      });
      toast.success('Schedule updated');
      setEditing(false);
    } catch {
      toast.error('Failed to update schedule');
    }
  }, [updateSchedule, sched.id, expr, prompt]);

  const handleToggle = useCallback(async () => {
    try {
      const newStatus = sched.status === 'active' ? 'paused' : 'active';
      await updateSchedule.mutateAsync({ id: sched.id, data: { status: newStatus } });
      toast.success(`Schedule ${newStatus}`);
    } catch {
      toast.error('Failed to update schedule');
    }
  }, [updateSchedule, sched.id, sched.status]);

  const handleDelete = useCallback(async () => {
    try {
      await deleteSchedule.mutateAsync(sched.id);
      toast.success('Schedule deleted');
    } catch {
      toast.error('Failed to delete schedule');
    }
    setConfirmDelete(false);
  }, [deleteSchedule, sched.id]);

  const handleTrigger = useCallback(async () => {
    try {
      await triggerSchedule.mutateAsync(sched.id);
      toast.success('Schedule triggered');
    } catch {
      toast.error('Failed to trigger schedule');
    }
    setConfirmTrigger(false);
  }, [triggerSchedule, sched.id]);

  return (
    <>
      <tr className="border-b border-sera-border/50 hover:bg-sera-surface-hover/50 transition-colors">
        <td className="py-3 px-4">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-sera-text">{sched.agentName}</span>
          </div>
        </td>
        <td className="py-3 px-4">
          <div className="flex items-center gap-2">
            <span className="text-sm text-sera-text">{sched.name}</span>
            {isManifest && (
              <Tooltip content="Declared in agent manifest — edit the manifest to change this schedule">
                <Info size={12} className="text-sera-text-dim cursor-help" />
              </Tooltip>
            )}
          </div>
        </td>
        <td className="py-3 px-4">
          <Badge variant="default">{sched.type}</Badge>
        </td>
        <td className="py-3 px-4">
          {editing ? (
            <div className="flex flex-col gap-1">
              <label htmlFor={`edit-expr-${sched.id}`} className="sr-only">
                Cron Expression
              </label>
              <Input
                id={`edit-expr-${sched.id}`}
                type="text"
                value={expr}
                onChange={(e) => setExpr(e.target.value)}
                className="text-xs font-mono w-40 h-8"
                autoFocus
              />
            </div>
          ) : (
            <span className="font-mono text-xs text-sera-accent">{sched.expression}</span>
          )}
        </td>
        <td className="py-3 px-4 text-xs text-sera-text-muted">
          {sched.nextRunAt ? formatDistanceToNow(sched.nextRunAt) : '—'}
        </td>
        <td className="py-3 px-4">
          <div className="flex items-center gap-2">
            {lastRunBadge(sched.lastRunStatus)}
            {sched.lastRunOutput && (
              <button
                onClick={() => setExpanded((e) => !e)}
                className="text-sera-text-dim hover:text-sera-text p-1 rounded-md transition-colors hover:bg-sera-surface-hover"
                aria-label="Toggle output"
                aria-expanded={expanded}
                aria-controls={`output-${sched.id}`}
              >
                {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              </button>
            )}
          </div>
        </td>
        <td className="py-3 px-4">{statusBadge(sched.status)}</td>
        <td className="py-3 px-4">
          <div className="flex items-center gap-1">
            {!isManifest && (
              <button
                onClick={() => {
                  void handleToggle();
                }}
                disabled={updateSchedule.isPending}
                className={`relative inline-flex h-4 w-8 cursor-pointer rounded-full transition-colors ${
                  sched.status === 'active'
                    ? 'bg-sera-success'
                    : 'bg-sera-surface-hover border border-sera-border'
                }`}
                title={sched.status === 'active' ? 'Pause schedule' : 'Activate schedule'}
                aria-label="Toggle status"
                aria-pressed={sched.status === 'active'}
              >
                <span
                  className={`inline-block h-3 w-3 mt-0.5 rounded-full bg-white shadow transition-transform ${
                    sched.status === 'active' ? 'translate-x-4' : 'translate-x-0.5'
                  }`}
                />
              </button>
            )}
            {editing ? (
              <>
                <button
                  onClick={() => {
                    void handleSave();
                  }}
                  className="text-sera-success hover:opacity-80 p-1 rounded-md transition-colors hover:bg-sera-success/10"
                  title="Save"
                  aria-label="Save changes"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => {
                    setEditing(false);
                    setExpr(sched.expression);
                    setPrompt(sched.taskPrompt ?? '');
                  }}
                  className="text-sera-text-dim hover:text-sera-text p-1 rounded-md transition-colors hover:bg-sera-surface-hover"
                  title="Cancel"
                  aria-label="Cancel editing"
                >
                  <X size={14} />
                </button>
              </>
            ) : (
              <>
                {!isManifest && (
                  <button
                    onClick={() => setEditing(true)}
                    className="p-1 text-sera-text-dim hover:text-sera-text transition-colors rounded-md hover:bg-sera-surface-hover"
                    title="Edit"
                    aria-label="Edit schedule"
                  >
                    <Pencil size={13} />
                  </button>
                )}
                <button
                  onClick={() => setConfirmTrigger(true)}
                  className="p-1 text-sera-text-dim hover:text-sera-success transition-colors rounded-md hover:bg-sera-success/10"
                  title="Run now"
                  aria-label="Run schedule now"
                >
                  <Play size={13} />
                </button>
                {!isManifest && (
                  <button
                    onClick={() => setConfirmDelete(true)}
                    className="p-1 text-sera-text-dim hover:text-sera-error transition-colors rounded-md hover:bg-sera-error/10"
                    title="Delete"
                    aria-label="Delete schedule"
                  >
                    <Trash2 size={13} />
                  </button>
                )}
              </>
            )}
          </div>
        </td>
      </tr>

      {/* Inline prompt edit row */}
      {editing && (
        <tr className="border-b border-sera-border/50 bg-sera-bg/30">
          <td colSpan={8} className="px-4 py-3">
            <div className="space-y-1">
              <label
                htmlFor={`edit-prompt-${sched.id}`}
                className="text-[11px] text-sera-text-dim uppercase tracking-wider"
              >
                Task Prompt
              </label>
              <textarea
                id={`edit-prompt-${sched.id}`}
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                rows={3}
                className="sera-input text-xs w-full resize-none focus:ring-1 focus:ring-sera-accent"
                placeholder="Prompt to run for this schedule…"
              />
            </div>
          </td>
        </tr>
      )}

      {/* Last run output */}
      {expanded && sched.lastRunOutput && (
        <tr
          id={`output-${sched.id}`}
          role="region"
          aria-label="Last run output"
          className="border-b border-sera-border/50 bg-sera-bg/50"
        >
          <td colSpan={8} className="px-8 py-3">
            <pre className="text-xs font-mono text-sera-text-muted leading-relaxed whitespace-pre-wrap">
              {/VIOLATES NOT NULL CONSTRAINT|syntax error|column .* does not exist/i.test(
                sched.lastRunOutput
              )
                ? 'Internal error: schedule configuration is invalid. Check agent logs for details.'
                : sched.lastRunOutput}
            </pre>
          </td>
        </tr>
      )}

      {/* Delete confirmation */}
      <Dialog open={confirmDelete} onOpenChange={(o: boolean) => !o && setConfirmDelete(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete schedule</DialogTitle>
            <DialogDescription>
              Delete schedule <strong>{sched.name}</strong> for agent{' '}
              <strong>{sched.agentName}</strong>? This cannot be undone.
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
              onClick={() => {
                void handleDelete();
              }}
              disabled={deleteSchedule.isPending}
            >
              Delete
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Trigger confirmation */}
      <Dialog open={confirmTrigger} onOpenChange={(o: boolean) => !o && setConfirmTrigger(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Run schedule now</DialogTitle>
            <DialogDescription>
              Trigger schedule <strong>{sched.name}</strong> immediately?
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
              variant="outline"
              onClick={() => {
                void handleTrigger();
              }}
              disabled={triggerSchedule.isPending}
            >
              <Play size={13} /> Run Now
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

export default function SchedulesPage() {
  const [agentFilter, setAgentFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState<'' | 'active' | 'paused'>('');
  const [showCreate, setShowCreate] = useState(false);
  const [newSchedule, setNewSchedule] = useState({
    agentName: '',
    name: '',
    expression: '',
    taskPrompt: '',
  });

  const { data: schedules, isLoading } = useSchedules({
    agentName: agentFilter || undefined,
    status: statusFilter || undefined,
  });
  const { data: agents } = useAgents();
  const createSchedule = useCreateSchedule();

  const agentNames = [...new Set((agents ?? []).map((a) => a.name))].sort();

  const handleCreate = useCallback(async () => {
    if (!newSchedule.agentName || !newSchedule.name || !newSchedule.expression) {
      toast.error('Agent, name, and cron expression are required');
      return;
    }
    try {
      await createSchedule.mutateAsync({
        agentName: newSchedule.agentName,
        name: newSchedule.name,
        type: 'cron',
        expression: newSchedule.expression,
        taskPrompt: newSchedule.taskPrompt || undefined,
        status: 'active',
      });
      toast.success('Schedule created');
      setShowCreate(false);
      setNewSchedule({ agentName: '', name: '', expression: '', taskPrompt: '' });
    } catch {
      toast.error('Failed to create schedule');
    }
  }, [createSchedule, newSchedule]);

  return (
    <main className="p-8 max-w-7xl mx-auto space-y-6">
      <header className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Schedules</h1>
          <p className="text-sm text-sera-text-muted mt-1" aria-live="polite">
            {schedules
              ? `${schedules.length} schedule${schedules.length !== 1 ? 's' : ''}`
              : 'Loading…'}
          </p>
        </div>
        <Button size="sm" onClick={() => setShowCreate(true)}>
          <Plus size={13} /> Create Schedule
        </Button>
      </header>

      {/* Create Schedule Dialog */}
      <Dialog open={showCreate} onOpenChange={(o: boolean) => !o && setShowCreate(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create Schedule</DialogTitle>
            <DialogDescription>Create a new cron schedule for an agent.</DialogDescription>
          </DialogHeader>
          <div className="space-y-3 mt-2">
            <div>
              <label htmlFor="create-agent" className="block text-xs text-sera-text-muted mb-1">
                Agent
              </label>
              <select
                id="create-agent"
                value={newSchedule.agentName}
                onChange={(e) => setNewSchedule((s) => ({ ...s, agentName: e.target.value }))}
                className="sera-input text-xs w-full focus:ring-1 focus:ring-sera-accent"
              >
                <option value="">Select agent…</option>
                {agentNames.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label htmlFor="create-name" className="block text-xs text-sera-text-muted mb-1">
                Name
              </label>
              <Input
                id="create-name"
                type="text"
                value={newSchedule.name}
                onChange={(e) => setNewSchedule((s) => ({ ...s, name: e.target.value }))}
                placeholder="e.g. Daily knowledge sync"
                className="text-xs w-full"
              />
            </div>
            <div>
              <label htmlFor="create-expr" className="block text-xs text-sera-text-muted mb-1">
                Cron Expression
              </label>
              <Input
                id="create-expr"
                type="text"
                value={newSchedule.expression}
                onChange={(e) => setNewSchedule((s) => ({ ...s, expression: e.target.value }))}
                placeholder="0 */6 * * *"
                className="text-xs w-full font-mono"
              />
              <p className="text-[10px] text-sera-text-dim mt-1">
                Standard 5-field cron: minute hour day month weekday
              </p>
            </div>
            <div>
              <label htmlFor="create-prompt" className="block text-xs text-sera-text-muted mb-1">
                Task Prompt (optional)
              </label>
              <textarea
                id="create-prompt"
                value={newSchedule.taskPrompt}
                onChange={(e) => setNewSchedule((s) => ({ ...s, taskPrompt: e.target.value }))}
                placeholder="What should the agent do when this schedule fires?"
                rows={3}
                className="sera-input text-xs w-full resize-none focus:ring-1 focus:ring-sera-accent"
              />
            </div>
          </div>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button
              size="sm"
              onClick={() => void handleCreate()}
              disabled={createSchedule.isPending}
            >
              Create
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Filters */}
      <section className="flex items-center gap-3 flex-wrap" aria-label="Filters">
        <div className="relative">
          <select
            value={agentFilter}
            onChange={(e) => setAgentFilter(e.target.value)}
            className="sera-input text-xs appearance-none pr-8 focus:ring-1 focus:ring-sera-accent"
            aria-label="Filter by agent"
          >
            <option value="">All agents</option>
            {agentNames.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
          <ChevronDown
            size={14}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-sera-text-dim pointer-events-none"
          />
        </div>

        <div
          className="flex items-center gap-1 border border-sera-border rounded-lg p-1"
          role="group"
          aria-label="Filter by status"
        >
          {(
            [
              ['', 'All'],
              ['active', 'Active'],
              ['paused', 'Paused'],
            ] as [string, string][]
          ).map(([val, label]) => (
            <button
              key={val}
              onClick={() => setStatusFilter(val as '' | 'active' | 'paused')}
              className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
                statusFilter === val
                  ? 'bg-sera-accent-soft text-sera-accent'
                  : 'text-sera-text-muted hover:text-sera-text'
              }`}
              aria-pressed={statusFilter === val}
            >
              {label}
            </button>
          ))}
        </div>
      </section>

      {/* Table */}
      <div className="sera-card-static overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm" aria-label="Schedules">
            <thead>
              <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                <th scope="col" className="text-left py-3 px-4">
                  Agent
                </th>
                <th scope="col" className="text-left py-3 px-4">
                  Name
                </th>
                <th scope="col" className="text-left py-3 px-4">
                  Type
                </th>
                <th scope="col" className="text-left py-3 px-4">
                  Expression
                </th>
                <th scope="col" className="text-left py-3 px-4">
                  Next Run
                </th>
                <th scope="col" className="text-left py-3 px-4">
                  Last Run
                </th>
                <th scope="col" className="text-left py-3 px-4">
                  Status
                </th>
                <th scope="col" className="py-3 px-4" aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {isLoading ? (
                Array.from({ length: 5 }).map((_, i) => (
                  <tr key={i} className="border-b border-sera-border/50">
                    {Array.from({ length: 8 }).map((_, j) => (
                      <td key={j} className="py-3 px-4">
                        <Skeleton className="h-4 w-full" />
                      </td>
                    ))}
                  </tr>
                ))
              ) : (schedules ?? []).length === 0 ? (
                <tr>
                  <td colSpan={8} className="py-12 text-center text-sera-text-dim text-sm">
                    No schedules found.
                  </td>
                </tr>
              ) : (
                (schedules ?? []).map((s) => <ScheduleRow key={s.id} sched={s} />)
              )}
            </tbody>
          </table>
        </div>
      </div>
    </main>
  );
}
