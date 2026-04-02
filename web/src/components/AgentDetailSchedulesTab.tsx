import { useState, useCallback } from 'react';
import { Calendar, Clock, Plus, Trash2, Play, Eraser, AlertTriangle } from 'lucide-react';
import { toast } from 'sonner';
import { useAgentSchedules, useClearStaleTasks } from '@/hooks/useAgents';
import { createSchedule, deleteSchedule, triggerSchedule } from '@/lib/api/schedules';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { TabLoading } from '@/components/AgentDetailTabLoading';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';

export function SchedulesTab({
  id,
  agentName: parentAgentName,
}: {
  id: string;
  agentName?: string;
}) {
  const { data: schedules, isLoading, refetch } = useAgentSchedules(id);
  const clearStaleTasks = useClearStaleTasks();
  const [showCreate, setShowCreate] = useState(false);
  const [creating, setCreating] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [forceTriggerId, setForceTriggerId] = useState<string | null>(null);
  const [newSchedule, setNewSchedule] = useState({
    name: '',
    expression: '',
    taskPrompt: '',
  });

  // We need the agent name from the schedule data or the parent — extract from existing schedules
  // For new schedules, we need to resolve agent name from ID
  const agentName = parentAgentName ?? schedules?.[0]?.agentName ?? '';

  const handleCreate = useCallback(async () => {
    if (!newSchedule.name.trim() || !newSchedule.expression.trim()) {
      toast.error('Name and cron expression are required');
      return;
    }
    setCreating(true);
    try {
      await createSchedule({
        agentName: agentName || id,
        name: newSchedule.name.trim(),
        type: 'cron',
        expression: newSchedule.expression.trim(),
        taskPrompt: newSchedule.taskPrompt.trim() || undefined,
        status: 'active',
      });
      toast.success('Schedule created');
      setShowCreate(false);
      setNewSchedule({ name: '', expression: '', taskPrompt: '' });
      void refetch();
    } catch {
      toast.error('Failed to create schedule');
    } finally {
      setCreating(false);
    }
  }, [newSchedule, agentName, id, refetch]);

  const handleDelete = async (schedId: string) => {
    try {
      await deleteSchedule(schedId);
      toast.success('Schedule deleted');
      void refetch();
    } catch {
      toast.error('Failed to delete schedule');
    }
    setConfirmDelete(null);
  };

  const handleTrigger = async (schedId: string, force = false) => {
    try {
      const res = await triggerSchedule(schedId, force);
      if (res.status === 'triggered') {
        toast.success('Schedule triggered');
        setForceTriggerId(null);
      } else if (res.status === 'skipped') {
        setForceTriggerId(schedId);
      }
    } catch (err: unknown) {
      if (err && typeof err === 'object' && 'status' in err && err.status === 409) {
        setForceTriggerId(schedId);
      } else {
        toast.error('Failed to trigger schedule');
      }
    }
  };

  const handleClearStale = async () => {
    try {
      const res = await clearStaleTasks.mutateAsync({ agentId: id });
      toast.success(`Cleared ${res.cleared} stale tasks`);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to clear stale tasks');
    }
  };

  if (isLoading) return <TabLoading />;

  return (
    <div className="p-6 space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-sera-text">
          Schedules{schedules?.length ? ` (${schedules.length})` : ''}
        </h2>
        <div className="flex gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => void handleClearStale()}
            disabled={clearStaleTasks.isPending}
            title="Clear tasks stuck in running state"
          >
            <Eraser size={12} /> Clear Stale
          </Button>
          <Button size="sm" variant="outline" onClick={() => setShowCreate(true)}>
            <Plus size={12} /> Add Schedule
          </Button>
        </div>
      </div>

      {!schedules?.length ? (
        <p className="text-sm text-sera-text-muted text-center py-8">No schedules configured.</p>
      ) : (
        <div className="space-y-2">
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
                  {sched.category && (
                    <span className="text-[10px] text-sera-text-dim px-1.5 py-0.5 rounded bg-sera-surface border border-sera-border">
                      {sched.category.replace(/_/g, ' ')}
                    </span>
                  )}
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
              <div className="flex items-center gap-1 flex-shrink-0">
                <button
                  onClick={() => void handleTrigger(sched.id)}
                  className="p-1.5 text-sera-text-dim hover:text-sera-accent transition-colors"
                  title="Trigger now"
                >
                  <Play size={12} />
                </button>
                <button
                  onClick={() => setConfirmDelete(sched.id)}
                  className="p-1.5 text-sera-text-dim hover:text-sera-error transition-colors"
                  title="Delete schedule"
                >
                  <Trash2 size={12} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Create Schedule Dialog */}
      <Dialog open={showCreate} onOpenChange={(o: boolean) => !o && setShowCreate(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create Schedule</DialogTitle>
            <DialogDescription>
              Add a recurring schedule for this agent using cron syntax.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 mt-2">
            <div>
              <label className="block text-xs text-sera-text-muted mb-1">Name</label>
              <Input
                value={newSchedule.name}
                onChange={(e) => setNewSchedule((s) => ({ ...s, name: e.target.value }))}
                placeholder="e.g. daily-report"
              />
            </div>
            <div>
              <label className="block text-xs text-sera-text-muted mb-1">Cron Expression</label>
              <Input
                value={newSchedule.expression}
                onChange={(e) => setNewSchedule((s) => ({ ...s, expression: e.target.value }))}
                placeholder="e.g. 0 9 * * 1-5 (weekdays at 9am)"
                className="font-mono"
              />
              <p className="text-[10px] text-sera-text-dim mt-1">
                Format: minute hour day-of-month month day-of-week
              </p>
            </div>
            <div>
              <label className="block text-xs text-sera-text-muted mb-1">
                Task Prompt (optional)
              </label>
              <textarea
                value={newSchedule.taskPrompt}
                onChange={(e) => setNewSchedule((s) => ({ ...s, taskPrompt: e.target.value }))}
                placeholder="What should the agent do when triggered?"
                rows={3}
                className="sera-input text-xs w-full resize-none"
              />
            </div>
          </div>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button size="sm" onClick={() => void handleCreate()} disabled={creating}>
              {creating ? 'Creating…' : 'Create'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Force Trigger Confirmation */}
      <Dialog open={!!forceTriggerId} onOpenChange={(o: boolean) => !o && setForceTriggerId(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="text-warning" size={18} />
              Schedule Skip Warning
            </DialogTitle>
            <DialogDescription>
              This schedule is being skipped because a task from it is already queued or running.
              Persistent agents process tasks one-by-one.
              <br />
              <br />
              Do you want to bypass this guard and enqueue the task anyway?
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
              variant="secondary"
              onClick={() => forceTriggerId && void handleTrigger(forceTriggerId, true)}
            >
              Force Trigger
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation */}
      <Dialog open={!!confirmDelete} onOpenChange={(o: boolean) => !o && setConfirmDelete(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Schedule</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete this schedule? This cannot be undone.
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
              onClick={() => confirmDelete && void handleDelete(confirmDelete)}
            >
              Delete
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
