import { useNavigate } from 'react-router';
import { Button } from '@/components/ui/button';

export function NotFoundView() {
  const navigate = useNavigate();
  return (
    <div className="flex h-screen flex-col items-center justify-center gap-4 p-8 bg-sera-bg text-center">
      <div className="text-6xl">🌌</div>
      <h1 className="text-2xl font-semibold text-sera-text">404 — Not Found</h1>
      <p className="max-w-md text-sm text-sera-text-muted">
        This page doesn&apos;t exist. Maybe the agent ate it.
      </p>
      <Button onClick={() => navigate('/')} variant="secondary">
        Back to Dashboard
      </Button>
    </div>
  );
}
