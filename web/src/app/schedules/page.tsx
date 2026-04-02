import { useState, useCallback } from 'react';
import { ChevronDown, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { useSchedules, useCreateSchedule } from '@/hooks/useSchedules';
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
import { useAgents } from '@/hooks/useAgents';
import { ScheduleRow } from '@/components/ScheduleRow';

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
