import { useParams, Link } from 'react-router';
import { ArrowLeft } from 'lucide-react';

export default function AgentEditPage() {
  const { id = '' } = useParams<{ id: string }>();

  return (
    <div className="p-6 max-w-2xl">
      <Link
        to={`/agents/${id}`}
        className="inline-flex items-center gap-1.5 text-xs text-sera-text-muted hover:text-sera-text mb-6 transition-colors"
      >
        <ArrowLeft size={12} /> Back
      </Link>
      <div className="sera-page-header">
        <h1 className="sera-page-title">Edit Agent</h1>
      </div>
      <p className="text-sm text-sera-text-muted">
        Instance editing (overrides patching) is not yet implemented. Stop and recreate the agent
        with different settings for now.
      </p>
    </div>
  );
}
