import { useParams } from 'react-router';
import { useMemoryGraph } from '@/hooks/useMemory';
import { Skeleton } from '@/components/ui/skeleton';

export default function MemoryDetailPage() {
  const { id } = useParams<{ id: string }>();
  const { data, isLoading } = useMemoryGraph();

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">Memory: {id}</h1>
      </div>
      {isLoading ? (
        <Skeleton className="h-64 rounded-xl" />
      ) : (
        <p className="text-sm text-sera-text-muted">
          Memory graph ({data?.nodes.length ?? 0} nodes) — full visualization coming in Epic 13
        </p>
      )}
    </div>
  );
}
