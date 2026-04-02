import { useState, useCallback } from 'react';
import { toast } from 'sonner';
import { useCreateSchedule } from '@/hooks/useSchedules';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';

interface CreateScheduleDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  agentNames: string[];
}

export function CreateScheduleDialog({
  open,
  onOpenChange,
  agentNames,
}: CreateScheduleDialogProps) {
  const [newSchedule, setNewSchedule] = useState({
    agentName: '',
    name: '',
    expression: '',
    taskPrompt: '',
  });

  const createSchedule = useCreateSchedule();

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
      onOpenChange(false);
      setNewSchedule({ agentName: '', name: '', expression: '', taskPrompt: '' });
    } catch {
      toast.error('Failed to create schedule');
    }
  }, [createSchedule, newSchedule, onOpenChange]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create Schedule</DialogTitle>
          <DialogDescription>Create a new cron schedule for an agent.</DialogDescription>
        </DialogHeader>
        <div className="space-y-3 mt-2">
          <div>
            <label className="block text-xs text-sera-text-muted mb-1">Agent</label>
            <select
              value={newSchedule.agentName}
              onChange={(e) => setNewSchedule((s) => ({ ...s, agentName: e.target.value }))}
              className="sera-input text-xs w-full"
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
            <label className="block text-xs text-sera-text-muted mb-1">Name</label>
            <input
              type="text"
              value={newSchedule.name}
              onChange={(e) => setNewSchedule((s) => ({ ...s, name: e.target.value }))}
              placeholder="e.g. Daily knowledge sync"
              className="sera-input text-xs w-full"
            />
          </div>
          <div>
            <label className="block text-xs text-sera-text-muted mb-1">Cron Expression</label>
            <input
              type="text"
              value={newSchedule.expression}
              onChange={(e) => setNewSchedule((s) => ({ ...s, expression: e.target.value }))}
              placeholder="0 */6 * * *"
              className="sera-input text-xs w-full font-mono"
            />
            <p className="text-[10px] text-sera-text-dim mt-1">
              Standard 5-field cron: minute hour day month weekday
            </p>
          </div>
          <div>
            <label className="block text-xs text-sera-text-muted mb-1">
              Task Prompt (optional)
            </label>
            <textarea
              value={newSchedule.taskPrompt}
              onChange={(e) => setNewSchedule((s) => ({ ...s, taskPrompt: e.target.value }))}
              placeholder="What should the agent do when this schedule fires?"
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
          <Button size="sm" onClick={() => void handleCreate()} disabled={createSchedule.isPending}>
            Create
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
