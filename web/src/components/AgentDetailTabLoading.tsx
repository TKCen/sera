import { Skeleton } from '@/components/ui/skeleton';

export function TabLoading() {
  return (
    <div className="p-6 space-y-3">
      <Skeleton className="h-6 w-full" />
      <Skeleton className="h-6 w-3/4" />
      <Skeleton className="h-6 w-1/2" />
    </div>
  );
}
