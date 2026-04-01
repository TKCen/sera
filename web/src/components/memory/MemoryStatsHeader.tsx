import { Database } from 'lucide-react';
import { useMemoryOverview, useRecentBlocks } from '@/hooks/useMemoryExplorer';
import { StatCard } from '@/components/StatCard';

export function MemoryStatsHeader() {
  const { data: overview, isLoading: overviewLoading } = useMemoryOverview();
  const { data: recentBlocks, isLoading: recentLoading } = useRecentBlocks(1);

  if (overviewLoading || recentLoading) {
    return (
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
        {[...Array(4)].map((_, i) => (
          <div key={i} className="h-24 sera-card-static animate-pulse bg-sera-surface-soft" />
        ))}
      </div>
    );
  }

  if (!overview) return null;

  const mostRecent = recentBlocks?.[0];

  return (
    <div className="space-y-6 mb-6">
      <div className="grid grid-cols-2 xl:grid-cols-4 gap-4">
        <StatCard
          label="Total Blocks"
          value={overview.totalBlocks.toString()}
          className="relative overflow-hidden min-w-0"
        />
        <StatCard
          label="Agents"
          value={overview.agents.length.toString()}
          className="relative overflow-hidden min-w-0"
        />
        <StatCard
          label="Top Tag"
          value={overview.topTags[0]?.tag || 'None'}
          className="relative overflow-hidden min-w-0"
        />
        <StatCard
          label="Updated"
          value={mostRecent ? new Date(mostRecent.timestamp).toLocaleDateString() : 'N/A'}
          className="relative overflow-hidden min-w-0"
        />
      </div>

      <div className="grid grid-cols-1 xl:grid-cols-3 gap-4">
        <div className="xl:col-span-2 sera-card-static p-4">
          <h3 className="text-sm font-medium text-sera-text-dim mb-4 flex items-center gap-2">
            <Database size={14} /> Memory Type Breakdown
          </h3>
          <div className="flex items-end gap-2 h-32">
            {Object.entries(overview.typeBreakdown).map(([type, count]) => {
              const percentage = (count / overview.totalBlocks) * 100;
              return (
                <div key={type} className="flex-1 flex flex-col items-center gap-2 group">
                  <div className="w-full relative flex flex-col justify-end h-full">
                     <div
                      className="w-full bg-sera-accent/20 border-t-2 border-sera-accent rounded-t-sm transition-all group-hover:bg-sera-accent/30"
                      style={{ height: `${percentage}%` }}
                    />
                    <div className="absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none">
                      <span className="text-[10px] bg-sera-bg/80 px-1 rounded border border-sera-border">
                        {count}
                      </span>
                    </div>
                  </div>
                  <span className="text-[10px] text-sera-text-dim truncate w-full text-center" title={type}>
                    {type}
                  </span>
                </div>
              );
            })}
          </div>
        </div>

        <div className="sera-card-static p-4 overflow-hidden">
          <h3 className="text-sm font-medium text-sera-text-dim mb-3">Top Tags</h3>
          <div className="flex flex-wrap gap-1.5">
            {overview.topTags.slice(0, 15).map(({ tag, count }) => (
              <div
                key={tag}
                className="text-[10px] px-2 py-0.5 rounded-full bg-sera-surface-hover border border-sera-border text-sera-text-muted"
              >
                {tag} <span className="text-sera-accent/70 ml-1">{count}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
