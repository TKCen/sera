import { useAgentTasks, useCancelTask } from '@/hooks/useAgents';
import { TabLoading } from './AgentDetailTabLoading';
import { Badge } from './ui/badge';
import { Button } from './ui/button';
import { CopyButton } from './CopyButton';
import { Clock, XCircle, AlertCircle, CheckCircle2, Timer } from 'lucide-react';
import { toast } from 'sonner';

export function AgentDetailTasksTab({ id }: { id: string }) {
  const { data: tasks, isLoading } = useAgentTasks(id);
  const cancelTask = useCancelTask();

  const handleCancel = async (taskId: string) => {
    try {
      await cancelTask.mutateAsync({ agentId: id, taskId });
      toast.success('Task cancellation requested');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to cancel task');
    }
  };

  if (isLoading) return <TabLoading />;

  return (
    <div className="p-6 space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-sera-text">
          Task Queue {tasks?.length ? `(${tasks.length})` : ''}
        </h2>
      </div>

      {!tasks?.length ? (
        <p className="text-sm text-sera-text-muted text-center py-8">No tasks found.</p>
      ) : (
        <div className="space-y-3">
          {tasks.map((task) => (
            <div key={task.id} className="sera-card-static p-4 flex items-start gap-4">
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-2">
                  <Badge
                    variant={
                      task.status === 'completed'
                        ? 'success'
                        : task.status === 'failed'
                          ? 'error'
                          : task.status === 'running'
                            ? 'warning'
                            : 'default'
                    }
                  >
                    {task.status}
                  </Badge>
                  <div className="flex items-center gap-1">
                    <span className="text-xs font-mono text-sera-text-dim truncate">{task.id}</span>
                    <CopyButton value={task.id} />
                  </div>
                  {task.exitReason && (
                    <Badge variant="default" className="text-[10px] uppercase">
                      {task.exitReason}
                    </Badge>
                  )}
                </div>

                <p className="text-sm text-sera-text font-medium line-clamp-2 mb-3">{task.task}</p>

                <div className="flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-sera-text-muted">
                  <span className="flex items-center gap-1">
                    <Clock size={12} /> Created: {new Date(task.createdAt).toLocaleString()}
                  </span>
                  {task.startedAt && (
                    <span className="flex items-center gap-1">
                      <Timer size={12} /> Started: {new Date(task.startedAt).toLocaleString()}
                    </span>
                  )}
                  {task.completedAt && (
                    <span className="flex items-center gap-1">
                      <CheckCircle2 size={12} /> Finished:{' '}
                      {new Date(task.completedAt).toLocaleString()}
                    </span>
                  )}
                </div>

                {task.error && (
                  <div className="mt-2 p-2 rounded bg-sera-error/10 border border-sera-error/20 flex items-start gap-2">
                    <AlertCircle size={12} className="text-sera-error mt-0.5 flex-shrink-0" />
                    <span className="text-xs text-sera-error line-clamp-2">{task.error}</span>
                  </div>
                )}
              </div>

              {(task.status === 'queued' || task.status === 'running') && (
                <Button
                  size="sm"
                  variant="outline"
                  className="text-sera-error hover:text-sera-error border-sera-error/20 hover:bg-sera-error/10"
                  onClick={() => handleCancel(task.id)}
                  disabled={cancelTask.isPending}
                >
                  <XCircle size={13} /> Cancel
                </Button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
