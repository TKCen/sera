import { useCallback } from 'react';
import { toast } from 'sonner';
import { useCreateSchedule } from '@/hooks/useSchedules';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
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
  newSchedule: {
    agentName: string;
    name: string;
    expression: string;
    taskPrompt: string;
  };
  onScheduleChange: (schedule: {
    agentName: string;
    name: string;
    expression: string;
    taskPrompt: string;
  }) => void;
}

export function CreateScheduleDialog({
  open,
  onOpenChange,
  agentNames,
  newSchedule,
  onScheduleChange,
}: CreateScheduleDialogProps) {
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
      onScheduleChange({ agentName: '', name: '', expression: '', taskPrompt: '' });
    } catch {
      toast.error('Failed to create schedule');
    }
  }, [createSchedule, newSchedule, onOpenChange, onScheduleChange]);

  return (
    <Dialog open={open} onOpenChange={(o: boolean) => !o && onOpenChange(false)}>
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
              onChange={(e) => onScheduleChange({ ...newSchedule, agentName: e.target.value })}
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
              onChange={(e) => onScheduleChange({ ...newSchedule, name: e.target.value })}
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
              onChange={(e) => onScheduleChange({ ...newSchedule, expression: e.target.value })}
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
              onChange={(e) => onScheduleChange({ ...newSchedule, taskPrompt: e.target.value })}
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
          <Button size="sm" onClick={() => void handleCreate()} disabled={createSchedule.isPending}>
            Create
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
