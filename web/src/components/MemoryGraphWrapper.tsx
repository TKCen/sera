import { Suspense, lazy } from 'react';
import type { MemoryGraphProps } from './MemoryGraph';

// In Vite, we use React.lazy instead of next/dynamic
const MemoryGraph = lazy(() => import('./MemoryGraph'));

export default function MemoryGraphWrapper(props: MemoryGraphProps) {
  return (
    <Suspense
      fallback={
        <div className="w-full h-[600px] flex items-center justify-center border border-sera-border rounded-lg bg-[#0a0a0a]">
          <div className="animate-pulse flex flex-col items-center gap-4 text-sera-text-muted">
            <div className="w-8 h-8 rounded-full border-2 border-sera-accent border-t-transparent animate-spin" />
            Loading graph visualization...
          </div>
        </div>
      }
    >
      <MemoryGraph {...props} />
    </Suspense>
  );
}
