import { useState } from 'react';
import { Link } from 'react-router';
import { Clock, ExternalLink } from 'lucide-react';
import { useAgentMemory } from '@/hooks/useAgents';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { TabLoading } from '@/components/AgentDetailTabLoading';

export function MemoryTab({ id }: { id: string }) {
  const [scope, setScope] = useState<string>('');
  const { data: blocks, isLoading } = useAgentMemory(id, scope || undefined);

  return (
    <div className="p-6 space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex gap-1">
          {(['', 'personal', 'circle', 'global'] as const).map((s) => (
            <button
              key={s}
              onClick={() => setScope(s)}
              className={cn(
                'px-3 py-1.5 rounded-md text-xs font-medium transition-colors',
                scope === s
                  ? 'bg-sera-accent-soft text-sera-accent'
                  : 'text-sera-text-muted hover:bg-sera-surface-hover'
              )}
            >
              {s === '' ? 'All' : s.charAt(0).toUpperCase() + s.slice(1)}
            </button>
          ))}
        </div>
        <Link
          to={`/agents/${id}/memory-graph`}
          className="flex items-center gap-1 text-xs text-sera-accent hover:underline"
        >
          <ExternalLink size={11} /> View graph
        </Link>
      </div>

      {isLoading ? (
        <TabLoading />
      ) : !blocks?.length ? (
        <p className="text-sm text-sera-text-muted text-center py-8">No memory blocks.</p>
      ) : (
        <div className="space-y-2">
          {blocks.map((block) => (
            <Link
              key={block.id}
              to={`/memory/${block.id}`}
              className="sera-card flex items-start gap-3 p-3 block"
            >
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-0.5">
                  <span className="text-sm font-medium text-sera-text truncate">{block.title}</span>
                  <Badge variant="accent">{block.type}</Badge>
                  <Badge variant="default">{block.scope}</Badge>
                </div>
                {block.tags && block.tags.length > 0 && (
                  <div className="flex gap-1 flex-wrap mt-1">
                    {block.tags.map((tag) => (
                      <span
                        key={tag}
                        className="text-[10px] text-sera-text-dim bg-sera-surface-active px-1.5 py-0.5 rounded"
                      >
                        {tag}
                      </span>
                    ))}
                  </div>
                )}
              </div>
              {block.updatedAt && (
                <span className="text-[10px] text-sera-text-dim flex-shrink-0 flex items-center gap-1 mt-0.5">
                  <Clock size={9} /> {new Date(block.updatedAt).toLocaleDateString()}
                </span>
              )}
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
