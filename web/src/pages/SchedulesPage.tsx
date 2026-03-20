import { useState, useCallback } from 'react';
import { Play, Pencil, Trash2, Check, X, Info, ChevronDown, ChevronRight } from 'lucide-react';
import { toast } from 'sonner';
import { formatDistanceToNow } from '@/lib/utils';
import {
  useSchedules,
  useUpdateSchedule,
  useDeleteSchedule,
  useTriggerSchedule,
} from '@/hooks/useSchedules';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
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
  return (
    <Badge variant={status === 'active' ? 'success' : 'default'}>
      {status}
    </Badge>
  );
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
      await updateSchedule.mutateAsync({ id: sched.id, data: { expression: expr, taskPrompt: prompt } });
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
            <input
              type="text"
              value={expr}
              onChange={(e) => setExpr(e.target.value)}
              className="sera-input text-xs font-mono w-40"
              autoFocus
            />
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
                className="text-sera-text-dim hover:text-sera-text"
              >
                {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              </button>
            )}
          </div>
        </td>
        <td className="py-3 px-4">
          {statusBadge(sched.status)}
        </td>
        <td className="py-3 px-4">
          <div className="flex items-center gap-1">
            {!isManifest && (
              <button
                onClick={() => { void handleToggle(); }}
                disabled={updateSchedule.isPending}
                className={`relative inline-flex h-4 w-8 cursor-pointer rounded-full transition-colors ${
                  sched.status === 'active' ? 'bg-sera-success' : 'bg-sera-surface-hover border border-sera-border'
                }`}
                title={sched.status === 'active' ? 'Pause schedule' : 'Activate schedule'}
              >
                <span className={`inline-block h-3 w-3 mt-0.5 rounded-full bg-white shadow transition-transform ${
                  sched.status === 'active' ? 'translate-x-4' : 'translate-x-0.5'
                }`} />
              </button>
            )}
            {editing ? (
              <>
                <button
                  onClick={() => { void handleSave(); }}
                  className="text-sera-success hover:opacity-80 p-1"
                  title="Save"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => { setEditing(false); setExpr(sched.expression); setPrompt(sched.taskPrompt ?? ''); }}
                  className="text-sera-text-dim hover:text-sera-text p-1"
                  title="Cancel"
                >
                  <X size={14} />
                </button>
              </>
            ) : (
              <>
                {!isManifest && (
                  <button
                    onClick={() => setEditing(true)}
                    className="p-1 text-sera-text-dim hover:text-sera-text transition-colors"
                    title="Edit"
                  >
                    <Pencil size={13} />
                  </button>
                )}
                <button
                  onClick={() => setConfirmTrigger(true)}
                  className="p-1 text-sera-text-dim hover:text-sera-success transition-colors"
                  title="Run now"
                >
                  <Play size={13} />
                </button>
                {!isManifest && (
                  <button
                    onClick={() => setConfirmDelete(true)}
                    className="p-1 text-sera-text-dim hover:text-sera-error transition-colors"
                    title="Delete"
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
              <label className="text-[11px] text-sera-text-dim uppercase tracking-wider">Task Prompt</label>
              <textarea
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                rows={3}
                className="sera-input text-xs w-full resize-none"
                placeholder="Prompt to run for this schedule…"
              />
            </div>
          </td>
        </tr>
      )}

      {/* Last run output */}
      {expanded && sched.lastRunOutput && (
        <tr className="border-b border-sera-border/50 bg-sera-bg/50">
          <td colSpan={8} className="px-8 py-3">
            <pre className="text-xs font-mono text-sera-text-muted leading-relaxed whitespace-pre-wrap">
              {sched.lastRunOutput}
            </pre>
          </td>
        </tr>
      )}

      {/* Delete confirmation */}
      <Dialog open={confirmDelete} onOpenChange={(o) => !o && setConfirmDelete(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete schedule</DialogTitle>
            <DialogDescription>
              Delete schedule <strong>{sched.name}</strong> for agent <strong>{sched.agentName}</strong>? This cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">Cancel</Button>
            </DialogClose>
            <Button size="sm" variant="danger" onClick={() => { void handleDelete(); }} disabled={deleteSchedule.isPending}>
              Delete
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Trigger confirmation */}
      <Dialog open={confirmTrigger} onOpenChange={(o) => !o && setConfirmTrigger(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Run schedule now</DialogTitle>
            <DialogDescription>
              Trigger schedule <strong>{sched.name}</strong> immediately?
            </DialogDescription>
          </DialogHeader>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">Cancel</Button>
            </DialogClose>
            <Button size="sm" variant="outline" onClick={() => { void handleTrigger(); }} disabled={triggerSchedule.isPending}>
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

  const { data: schedules, isLoading } = useSchedules({
    agentName: agentFilter || undefined,
    status: statusFilter || undefined,
  });
  const { data: agents } = useAgents();

  const agentNames = [...new Set((agents ?? []).map((a) => a.metadata.name))].sort();

  return (
    <div className="p-8 max-w-7xl mx-auto space-y-6">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Schedules</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            {schedules ? `${schedules.length} schedule${schedules.length !== 1 ? 's' : ''}` : 'Loading…'}
          </p>
        </div>
      </div>

      {/* Filters */}
      <div className="flex items-center gap-3 flex-wrap">
        <select
          value={agentFilter}
          onChange={(e) => setAgentFilter(e.target.value)}
          className="sera-input text-xs appearance-none pr-6"
        >
          <option value="">All agents</option>
          {agentNames.map((n) => (
            <option key={n} value={n}>{n}</option>
          ))}
        </select>

        <div className="flex items-center gap-1 border border-sera-border rounded-lg p-1">
          {([['', 'All'], ['active', 'Active'], ['paused', 'Paused']] as [string, string][]).map(([val, label]) => (
            <button
              key={val}
              onClick={() => setStatusFilter(val as '' | 'active' | 'paused')}
              className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
                statusFilter === val
                  ? 'bg-sera-accent-soft text-sera-accent'
                  : 'text-sera-text-muted hover:text-sera-text'
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {/* Table */}
      <div className="sera-card-static overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                <th className="text-left py-3 px-4">Agent</th>
                <th className="text-left py-3 px-4">Name</th>
                <th className="text-left py-3 px-4">Type</th>
                <th className="text-left py-3 px-4">Expression</th>
                <th className="text-left py-3 px-4">Next Run</th>
                <th className="text-left py-3 px-4">Last Run</th>
                <th className="text-left py-3 px-4">Status</th>
                <th className="py-3 px-4" />
              </tr>
            </thead>
            <tbody>
              {isLoading ? (
                Array.from({ length: 5 }).map((_, i) => (
                  <tr key={i} className="border-b border-sera-border/50">
                    {Array.from({ length: 8 }).map((_, j) => (
                      <td key={j} className="py-3 px-4"><Skeleton className="h-4 w-full" /></td>
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
    </div>
  );
}
