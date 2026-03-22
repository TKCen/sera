import { Calendar, Clock } from 'lucide-react';
import { useAgentSchedules } from '@/hooks/useAgents';
import { Badge } from '@/components/ui/badge';
import { TabLoading } from '@/components/AgentDetailTabLoading';

export function SchedulesTab({ id }: { id: string }) {
  const { data: schedules, isLoading } = useAgentSchedules(id);

  if (isLoading) return <TabLoading />;

  if (!schedules?.length) {
    return (
      <div className="p-6">
        <p className="text-sm text-sera-text-muted text-center py-8">No schedules configured.</p>
      </div>
    );
  }

  return (
    <div className="p-6 space-y-2">
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
        </div>
      ))}
    </div>
  );
}
