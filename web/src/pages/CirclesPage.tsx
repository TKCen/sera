import { useCircles } from '@/hooks/useCircles';
import { Users } from 'lucide-react';
import { EmptyState } from '@/components/EmptyState';
import { Skeleton } from '@/components/ui/skeleton';
import { Link } from 'react-router';

export default function CirclesPage() {
  const { data: circles, isLoading } = useCircles();

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">Circles</h1>
      </div>
      {isLoading ? (
        <div className="space-y-3">
          {[1, 2].map((i) => (
            <Skeleton key={i} className="h-16 rounded-xl" />
          ))}
        </div>
      ) : !circles?.length ? (
        <EmptyState
          icon={<Users size={24} />}
          title="No circles"
          description="Create a circle to group agents."
        />
      ) : (
        <div className="space-y-2">
          {circles.map((c) => (
            <Link
              key={c.name}
              to={`/circles/${c.name}`}
              className="sera-card flex items-center gap-4 px-4 py-3 block"
            >
              <div className="h-9 w-9 rounded-lg bg-sera-accent-soft flex items-center justify-center">
                <Users size={16} className="text-sera-accent" />
              </div>
              <div>
                <div className="font-medium text-sm text-sera-text">{c.displayName ?? c.name}</div>
                <div className="text-xs text-sera-text-muted">{c.memberCount ?? 0} members</div>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
