import { useState } from 'react';
import { Plus } from 'lucide-react';
import { useSchedules } from '@/hooks/useSchedules';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { useAgents } from '@/hooks/useAgents';
import { CreateScheduleDialog } from '@/components/CreateScheduleDialog';
import { ScheduleRow } from '@/components/ScheduleRow';

export default function SchedulesPage() {
  const [agentFilter, setAgentFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState<'' | 'active' | 'paused'>('');
  const [showCreate, setShowCreate] = useState(false);

  const { data: schedules, isLoading } = useSchedules({
    agentName: agentFilter || undefined,
    status: statusFilter || undefined,
  });
  const { data: agents } = useAgents();

  const agentNames = [...new Set((agents ?? []).map((a) => a.name))].sort();

  return (
    <div className="p-8 max-w-7xl mx-auto space-y-6">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Schedules</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            {schedules
              ? `${schedules.length} schedule${schedules.length !== 1 ? 's' : ''}`
              : 'Loading…'}
          </p>
        </div>
        <Button size="sm" onClick={() => setShowCreate(true)}>
          <Plus size={13} /> Create Schedule
        </Button>
      </div>

      <CreateScheduleDialog
        open={showCreate}
        onOpenChange={setShowCreate}
        agentNames={agentNames}
      />

      {/* Filters */}
      <div className="flex items-center gap-3 flex-wrap">
        <select
          value={agentFilter}
          onChange={(e) => setAgentFilter(e.target.value)}
          className="sera-input text-xs appearance-none pr-6"
        >
          <option value="">All agents</option>
          {agentNames.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>

        <div className="flex items-center gap-1 border border-sera-border rounded-lg p-1">
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
    </div>
  );
}
