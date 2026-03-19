import { useParams } from 'react-router';
import { useAgent } from '@/hooks/useAgents';
import { Skeleton } from '@/components/ui/skeleton';

export default function AgentDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, isLoading } = useAgent(id ?? '');

  if (isLoading) {
    return (
      <div className="p-6 space-y-4">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-32 rounded-xl" />
      </div>
    );
  }

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">{data?.name ?? id}</h1>
      </div>
      <p className="text-sm text-sera-text-muted">Agent detail — coming in Epic 13</p>
    </div>
  );
}
