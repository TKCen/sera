import { Clock } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import type { ScopedBlock } from '@/lib/api/memory';

const TYPE_COLORS: Record<string, string> = {
  fact: 'bg-blue-500/15 text-blue-400',
  context: 'bg-purple-500/15 text-purple-400',
  memory: 'bg-green-500/15 text-green-400',
  insight: 'bg-yellow-500/15 text-yellow-400',
  reference: 'bg-cyan-500/15 text-cyan-400',
  observation: 'bg-orange-500/15 text-orange-400',
  decision: 'bg-red-500/15 text-red-400',
};

interface BlockCardProps {
  block: ScopedBlock;
  showAgent?: boolean;
  selected?: boolean;
  onClick?: () => void;
}

export function BlockCard({ block, showAgent, selected, onClick }: BlockCardProps) {
  const typeColor = TYPE_COLORS[block.type] ?? 'bg-sera-surface text-sera-text-muted';

  return (
    <button
      type="button"
      onClick={onClick}
      className={`w-full text-left p-3 rounded-lg border transition-colors ${
        selected
          ? 'border-sera-accent bg-sera-accent/10'
          : 'border-sera-border bg-sera-surface hover:border-sera-accent/50'
      }`}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span className={`text-[11px] px-1.5 py-0.5 rounded font-medium ${typeColor}`}>
              {block.type}
            </span>
            {showAgent && (
              <span className="text-[10px] text-sera-text-dim truncate">{block.agentId}</span>
            )}
          </div>
          <p className="text-sm font-medium text-sera-text truncate">{block.title}</p>
          {block.content && (
            <p className="text-xs text-sera-text-muted line-clamp-2 mt-1">
              {block.content.slice(0, 120)}
            </p>
          )}
        </div>
      </div>
      {(block.tags.length > 0 || block.timestamp) && (
        <div className="flex items-center gap-2 mt-2 flex-wrap">
          {block.tags.slice(0, 3).map((tag) => (
            <Badge key={tag} variant="default" className="text-[10px]">
              {tag}
            </Badge>
          ))}
          {block.tags.length > 3 && (
            <span className="text-[10px] text-sera-text-dim">+{block.tags.length - 3}</span>
          )}
          <span className="flex items-center gap-1 text-[10px] text-sera-text-dim ml-auto">
            <Clock size={10} />
            {new Date(block.timestamp).toLocaleDateString()}
          </span>
        </div>
      )}
    </button>
  );
}
