import { Link } from 'react-router';
import { Brain, FileText, Search, ExternalLink } from 'lucide-react';
import { useAgentStats } from '@/hooks/useMemoryExplorer';
import { Button } from '@/components/ui/button';
import { TabLoading } from '@/components/AgentDetailTabLoading';

export function MemoryTab({ id }: { id: string }) {
  const { data: stats, isLoading } = useAgentStats(id);

  if (isLoading) {
    return <TabLoading />;
  }

  return (
    <div className="p-6">
      <div className="sera-card-static p-8 flex flex-col items-center text-center space-y-6">
        <div className="w-16 h-16 rounded-full bg-sera-accent/10 flex items-center justify-center">
          <Brain size={32} className="text-sera-accent" />
        </div>

        <div>
          <h3 className="text-lg font-semibold text-sera-text">Agent Memory</h3>
          <p className="text-sm text-sera-text-muted mt-1 max-w-sm">
            This agent manages its own long-term memory, including facts, preferences, and past experiences.
          </p>
        </div>

        <div className="grid grid-cols-2 gap-4 w-full max-w-md">
          <div className="bg-sera-surface border border-sera-border rounded-xl p-4">
            <div className="flex items-center gap-2 text-sera-text-dim mb-1 justify-center">
              <FileText size={14} />
              <span className="text-[10px] uppercase font-bold tracking-wider">Blocks</span>
            </div>
            <div className="text-2xl font-bold text-sera-text">{stats?.blockCount ?? 0}</div>
          </div>
          <div className="bg-sera-surface border border-sera-border rounded-xl p-4">
            <div className="flex items-center gap-2 text-sera-text-dim mb-1 justify-center">
              <Search size={14} />
              <span className="text-[10px] uppercase font-bold tracking-wider">Vectors</span>
            </div>
            <div className="text-2xl font-bold text-sera-text">{stats?.vectorCount ?? 0}</div>
          </div>
        </div>

        <div className="flex flex-col gap-3 w-full max-w-xs">
          <Button asChild className="w-full">
            <Link to={`/memory?agent=${id}`}>
              <ExternalLink size={14} className="mr-2" />
              Open Memory Explorer
            </Link>
          </Button>
          <p className="text-[10px] text-sera-text-dim">
            Use the explorer to search, visualize, and manage individual memory blocks.
          </p>
        </div>
      </div>
    </div>
  );
}
