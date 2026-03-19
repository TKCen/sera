import { useParams } from 'react-router';

export default function AgentEditPage() {
  const { id } = useParams<{ id: string }>();
  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">Edit Agent: {id}</h1>
      </div>
      <p className="text-sm text-sera-text-muted">Agent editor — coming in Epic 13</p>
    </div>
  );
}
