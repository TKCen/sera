import { useParams, Link } from 'react-router';
import { ArrowLeft } from 'lucide-react';
import { AgentForm } from '@/components/AgentForm';
import { useAgents } from '@/hooks/useAgents';
import { Skeleton } from '@/components/ui/skeleton';

export default function AgentEditPage() {
  const { id = '' } = useParams<{ id: string }>();
  const { data: agents, isLoading } = useAgents();
  const manifest = agents?.find((a) => a.metadata.name === id);

  return (
    <div className="p-6 max-w-2xl">
      <Link
        to={`/agents/${id}`}
        className="inline-flex items-center gap-1.5 text-xs text-sera-text-muted hover:text-sera-text mb-6 transition-colors"
      >
        <ArrowLeft size={12} /> {id}
      </Link>
      <div className="sera-page-header">
        <h1 className="sera-page-title">Edit Agent: {id}</h1>
      </div>
      {isLoading ? (
        <div className="space-y-4">
          <Skeleton className="h-10 rounded-xl" />
          <Skeleton className="h-32 rounded-xl" />
        </div>
      ) : manifest ? (
        <AgentForm initial={manifest} isEdit />
      ) : (
        <p className="text-sm text-sera-text-muted">Agent not found.</p>
      )}
    </div>
  );
}
