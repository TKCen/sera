import { useState, useMemo } from 'react';
import { useAgents } from '@/hooks/useAgents';
import { useCircles } from '@/hooks/useCircles';
import { useIntrospection, type IntrospectionView } from '@/hooks/useIntrospection';
import { IntrospectionSidebar } from '@/components/introspection/IntrospectionSidebar';
import { IntrospectionFeed } from '@/components/introspection/IntrospectionFeed';
import { Skeleton } from '@/components/ui/skeleton';

function IntrospectionPageContent() {
  const { data: agents, isLoading: agentsLoading } = useAgents();
  const { data: circles, isLoading: circlesLoading } = useCircles();
  const [view, setView] = useState<IntrospectionView>({ kind: 'global' });

  const { messages } = useIntrospection(view, agents ?? []);

  // Get title based on current view
  const pageTitle = useMemo(() => {
    switch (view.kind) {
      case 'global':
        return 'Global Feed';
      case 'circle': {
        const circle = circles?.find((c) => c.name === view.circleId);
        return circle ? `Circle: ${circle.displayName}` : 'Circle Feed';
      }
      case 'agent':
        return `Agent: ${view.agentName}`;
    }
  }, [view, circles]);

  if (agentsLoading || circlesLoading) {
    return (
      <div className="flex h-full">
        <div className="w-64 border-r border-sera-border bg-sera-surface p-4 space-y-3">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-6 w-24" />
          <Skeleton className="h-8 w-full" />
          <Skeleton className="h-8 w-full" />
        </div>
        <div className="flex-1 flex flex-col">
          <div className="px-4 py-3 border-b border-sera-border">
            <Skeleton className="h-6 w-32" />
          </div>
          <div className="flex-1 overflow-auto p-4 space-y-3">
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      <IntrospectionSidebar
        circles={circles ?? []}
        agents={agents ?? []}
        activeView={view}
        onViewChange={setView}
      />
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Header */}
        <header className="px-4 py-3 border-b border-sera-border bg-sera-surface-active">
          <h1 className="text-lg font-semibold text-sera-text">{pageTitle}</h1>
        </header>

        {/* Feed */}
        <IntrospectionFeed messages={messages} />
      </div>
    </div>
  );
}

export default IntrospectionPageContent;
