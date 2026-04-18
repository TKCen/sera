import { Clock } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { MEMORY_TYPE_DOT } from '@/components/memory/constants';
import type { ScopedBlock } from '@/lib/api/memory';

interface TimelineViewProps {
  blocks: ScopedBlock[];
  selectedBlockId?: string | null;
  onBlockClick: (block: ScopedBlock) => void;
  agentNameMap?: Map<string, string>;
}

/** Group blocks by calendar day, sorted newest first. */
function groupByDay(blocks: ScopedBlock[]): Map<string, ScopedBlock[]> {
  const groups = new Map<string, ScopedBlock[]>();
  for (const block of blocks) {
    const day = new Date(block.timestamp).toLocaleDateString('en-US', {
      weekday: 'long',
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    });
    const existing = groups.get(day);
    if (existing) {
      existing.push(block);
    } else {
      groups.set(day, [block]);
    }
  }
  return groups;
}

export function TimelineView({
  blocks,
  selectedBlockId,
  onBlockClick,
  agentNameMap,
}: TimelineViewProps) {
  const groups = groupByDay(blocks);

  if (blocks.length === 0) {
    return <p className="text-sm text-sera-text-muted text-center py-8">No blocks to display</p>;
  }

  return (
    <div className="space-y-6">
      {[...groups.entries()].map(([day, dayBlocks]) => (
        <div key={day}>
          {/* Day header */}
          <div className="flex items-center gap-3 mb-3">
            <div className="w-3 h-3 rounded-full bg-sera-accent shrink-0" />
            <h3 className="text-sm font-semibold text-sera-text">{day}</h3>
            <div className="flex-1 h-px bg-sera-border" />
            <span className="text-[10px] text-sera-text-dim">
              {dayBlocks.length} block{dayBlocks.length !== 1 ? 's' : ''}
            </span>
          </div>

          {/* Timeline entries */}
          <div className="ml-1.5 border-l-2 border-sera-border pl-5 space-y-2">
            {dayBlocks.map((block) => {
              const dotColor = MEMORY_TYPE_DOT[block.type] ?? 'bg-sera-text-dim';
              const isSelected = selectedBlockId === block.id;
              return (
                <button
                  key={block.id}
                  type="button"
                  onClick={() => onBlockClick(block)}
                  className={`relative w-full text-left p-2.5 rounded-lg border transition-colors ${
                    isSelected
                      ? 'border-sera-accent bg-sera-accent/10'
                      : 'border-transparent hover:border-sera-border hover:bg-sera-surface/50'
                  }`}
                >
                  {/* Timeline dot */}
                  <div
                    className={`absolute -left-[1.625rem] top-3.5 w-2.5 h-2.5 rounded-full border-2 border-sera-bg ${dotColor}`}
                  />

                  <div className="flex items-center gap-2 mb-1">
                    <Badge variant="accent" className="text-[10px]">
                      {block.type}
                    </Badge>
                    {agentNameMap && (
                      <span className="text-[10px] text-sera-text-dim">
                        {agentNameMap.get(block.agentId) ?? block.agentId}
                      </span>
                    )}
                    <span className="text-[10px] text-sera-text-dim ml-auto flex items-center gap-1">
                      <Clock size={10} />
                      {new Date(block.timestamp).toLocaleTimeString('en-US', {
                        hour: '2-digit',
                        minute: '2-digit',
                      })}
                    </span>
                  </div>

                  <p className="text-sm font-medium text-sera-text truncate">{block.title}</p>

                  {block.tags.length > 0 && (
                    <div className="flex gap-1 flex-wrap mt-1">
                      {block.tags.slice(0, 4).map((tag) => (
                        <span
                          key={tag}
                          className="text-[9px] bg-sera-surface text-sera-text-muted px-1 py-0.5 rounded"
                        >
                          {tag}
                        </span>
                      ))}
                    </div>
                  )}
                </button>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
