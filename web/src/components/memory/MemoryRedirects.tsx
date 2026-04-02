import { Navigate, useParams } from 'react-router';

export function AgentMemoryGraphRedirect() {
  const { id } = useParams<{ id: string }>();
  return <Navigate to={`/memory?agent=${id}`} replace />;
}

export function MemoryDetailRedirect() {
  const { id } = useParams<{ id: string }>();
  return <Navigate to={`/memory?agent=${id}`} replace />;
}
