import { useState } from 'react';
import { Database, ChevronDown, LayoutList, Clock, Archive } from 'lucide-react';
import { toast } from 'sonner';
import {
  useMemoryOverview,
  useRecentBlocks,
  useAgentBlockList,
  useTriggerCompaction,
} from '@/hooks/useMemoryExplorer';
import { BlockCard } from './BlockCard';
import { TimelineView } from './TimelineView';
import { TagCloud } from './TagCloud';
import { Spinner } from '@/components/ui/spinner';
import type { ScopedBlock } from '@/lib/api/memory';

export type MemoryScope =
  | { kind: 'global' }
  | { kind: 'agent'; agentId: string }
  | { kind: 'circle'; circleId: string };

interface MemorySidebarProps {
  scope: MemoryScope;
  onScopeChange: (scope: MemoryScope) => void;
  selectedBlockId: string | null;
  onBlockSelect: (block: ScopedBlock) => void;
  tagFilter: string;
  onTagFilter: (tag: string) => void;
  agentNameMap?: Map<string, string>;
}

export function MemorySidebar({
  scope,
  onScopeChange,
  selectedBlockId,
  onBlockSelect,
  tagFilter,
  onTagFilter,
  agentNameMap,
}: MemorySidebarProps) {
  const [typeFilter, setTypeFilter] = useState('');
  const [viewMode, setViewMode] = useState<'cards' | 'timeline'>('cards');
  const compactMutation = useTriggerCompaction();
  const { data: overview, isLoading: overviewLoading } = useMemoryOverview();
  const { data: recentBlocks } = useRecentBlocks(50);
  const agentId = scope.kind === 'agent' ? scope.agentId : '';
  const { data: agentBlocks } = useAgentBlockList(agentId);

  // Determine blocks to show based on scope
  const blocks = scope.kind === 'agent' ? (agentBlocks ?? []) : (recentBlocks ?? []);

  // Apply filters
  const filteredBlocks = blocks.filter((b) => {
    if (typeFilter && b.type !== typeFilter) return false;
    if (tagFilter && !b.tags.includes(tagFilter)) return false;
    return true;
  });

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Scope selector */}
      <div className="p-3 border-b border-sera-border">
        <div className="relative">
          <select
            value={
              scope.kind === 'global'
                ? 'global'
                : scope.kind === 'agent'
                  ? `agent:${scope.agentId}`
                  : `circle:${scope.circleId}`
            }
            onChange={(e) => {
              const val = e.target.value;
              if (val === 'global') onScopeChange({ kind: 'global' });
              else if (val.startsWith('agent:'))
                onScopeChange({ kind: 'agent', agentId: val.slice(6) });
              else if (val.startsWith('circle:'))
                onScopeChange({ kind: 'circle', circleId: val.slice(7) });
            }}
            className="sera-input w-full text-sm pr-8 appearance-none"
          >
            <option value="global">Global (all agents)</option>
            {overview?.agents.map((a) => (
              <option key={a.id} value={`agent:${a.id}`}>
                {agentNameMap?.get(a.id) ?? a.id} ({a.blockCount} blocks)
              </option>
            ))}
          </select>
          <ChevronDown
            size={14}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-sera-text-dim pointer-events-none"
          />
        </div>
      </div>

      {/* Stats */}
      {overviewLoading ? (
        <div className="p-4 flex justify-center">
          <Spinner size="sm" />
        </div>
      ) : overview ? (
        <div className="p-3 border-b border-sera-border">
          <div className="grid grid-cols-2 gap-2 text-center">
            <div className="sera-card-static p-2">
              <div className="text-lg font-bold text-sera-accent">{overview.totalBlocks}</div>
              <div className="text-[10px] text-sera-text-dim uppercase">Blocks</div>
            </div>
            <div className="sera-card-static p-2">
              <div className="text-lg font-bold text-sera-text">{overview.agents.length}</div>
              <div className="text-[10px] text-sera-text-dim uppercase">Agents</div>
            </div>
          </div>

          {/* Type filter pills */}
          {Object.keys(overview.typeBreakdown).length > 0 && (
            <div className="flex gap-1 flex-wrap mt-2">
              <button
                type="button"
                onClick={() => setTypeFilter('')}
                className={`text-[10px] px-1.5 py-0.5 rounded ${
                  !typeFilter
                    ? 'bg-sera-accent/20 text-sera-accent'
                    : 'text-sera-text-muted hover:text-sera-text'
                }`}
              >
                All
              </button>
              {Object.entries(overview.typeBreakdown).map(([type, count]) => (
                <button
                  key={type}
                  type="button"
                  onClick={() => setTypeFilter(typeFilter === type ? '' : type)}
                  className={`text-[10px] px-1.5 py-0.5 rounded ${
                    typeFilter === type
                      ? 'bg-sera-accent/20 text-sera-accent'
                      : 'text-sera-text-muted hover:text-sera-text'
                  }`}
                >
                  {type} ({count})
                </button>
              ))}
            </div>
          )}
          {/* Compact button (agent-scoped only) */}
          {scope.kind === 'agent' && (
            <button
              type="button"
              disabled={compactMutation.isPending}
              onClick={async () => {
                try {
                  await compactMutation.mutateAsync(scope.agentId);
                  toast.success('Memory compaction complete');
                } catch (err) {
                  toast.error(
                    `Compaction failed: ${err instanceof Error ? err.message : String(err)}`
                  );
                }
              }}
              className="mt-2 w-full flex items-center justify-center gap-1.5 text-[10px] px-2 py-1.5 rounded border border-sera-border text-sera-text-muted hover:text-sera-text hover:border-sera-accent/50 transition-colors"
            >
              <Archive size={10} />
              {compactMutation.isPending ? 'Compacting...' : 'Compact Memory'}
            </button>
          )}
        </div>
      ) : null}

      {/* Tag cloud */}
      {overview && overview.topTags.length > 0 && (
        <div className="p-3 border-b border-sera-border">
          <div className="sera-section-label mb-1.5 flex items-center gap-1">
            <Database size={12} /> Tags
          </div>
          <TagCloud tags={overview.topTags} activeTag={tagFilter} onTagClick={onTagFilter} />
        </div>
      )}

      {/* Block list header + view toggle */}
      <div className="flex items-center justify-between px-3 pt-2 pb-1">
        <span className="sera-section-label">Blocks ({filteredBlocks.length})</span>
        <div className="flex gap-0.5 bg-sera-surface rounded p-0.5">
          <button
            type="button"
            onClick={() => setViewMode('cards')}
            className={`p-1 rounded transition-colors ${
              viewMode === 'cards'
                ? 'bg-sera-accent/20 text-sera-accent'
                : 'text-sera-text-dim hover:text-sera-text'
            }`}
            title="Card view"
          >
            <LayoutList size={12} />
          </button>
          <button
            type="button"
            onClick={() => setViewMode('timeline')}
            className={`p-1 rounded transition-colors ${
              viewMode === 'timeline'
                ? 'bg-sera-accent/20 text-sera-accent'
                : 'text-sera-text-dim hover:text-sera-text'
            }`}
            title="Timeline view"
          >
            <Clock size={12} />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-2 space-y-1.5">
        {filteredBlocks.length === 0 ? (
          <p className="text-sm text-sera-text-muted text-center py-8">No blocks found</p>
        ) : viewMode === 'timeline' ? (
          <TimelineView
            blocks={filteredBlocks}
            selectedBlockId={selectedBlockId}
            onBlockClick={onBlockSelect}
            agentNameMap={agentNameMap}
          />
        ) : (
          filteredBlocks.map((block) => (
            <BlockCard
              key={block.id}
              block={block}
              showAgent={scope.kind === 'global'}
              agentName={agentNameMap?.get(block.agentId)}
              selected={selectedBlockId === block.id}
              onClick={() => onBlockSelect(block)}
            />
          ))
        )}
      </div>
    </div>
  );
}
