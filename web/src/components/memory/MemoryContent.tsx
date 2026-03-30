import { useState, useCallback } from 'react';
import { Search, ArrowUpRight, ArrowDownLeft, Save, Brain } from 'lucide-react';
import { toast } from 'sonner';
import { useBlockDetail, useBlockBacklinks, useMemorySearch } from '@/hooks/useMemoryExplorer';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { EmptyState } from '@/components/EmptyState';
import { BlockCard } from './BlockCard';
import type { ScopedBlock } from '@/lib/api/memory';

interface MemoryContentProps {
  selectedAgentId: string;
  selectedBlockId: string;
  onBlockSelect: (block: ScopedBlock) => void;
}

export function MemoryContent({
  selectedAgentId,
  selectedBlockId,
  onBlockSelect,
}: MemoryContentProps) {
  const [searchQuery, setSearchQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');

  const { data: block, isLoading: blockLoading } = useBlockDetail(selectedAgentId, selectedBlockId);
  const { data: backlinks } = useBlockBacklinks(selectedAgentId, selectedBlockId);
  const { data: searchResults, isLoading: searchLoading } = useMemorySearch(debouncedQuery);

  const handleSearchChange = useCallback((value: string) => {
    setSearchQuery(value);
    // Simple debounce via setTimeout
    const timer = setTimeout(() => setDebouncedQuery(value), 300);
    return () => clearTimeout(timer);
  }, []);

  const isSearching = debouncedQuery.length >= 2;
  const hasBlock = selectedBlockId.length > 0 && !isSearching;

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Search bar */}
      <div className="p-3 border-b border-sera-border">
        <div className="relative">
          <Search
            size={14}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-sera-text-dim"
          />
          <input
            type="text"
            placeholder="Search memory..."
            value={searchQuery}
            onChange={(e) => handleSearchChange(e.target.value)}
            className="sera-input w-full pl-9 text-sm"
          />
        </div>
      </div>

      {/* Content area */}
      <div className="flex-1 overflow-y-auto p-4">
        {isSearching ? (
          <SearchResults
            results={searchResults ?? []}
            loading={searchLoading}
            query={debouncedQuery}
            onBlockSelect={onBlockSelect}
          />
        ) : hasBlock ? (
          <BlockDetail
            block={block ?? null}
            loading={blockLoading}
            backlinks={backlinks ?? []}
            onBlockSelect={onBlockSelect}
          />
        ) : (
          <EmptyState
            icon={<Brain size={24} />}
            title="Select a memory block"
            description="Choose a block from the sidebar or search to view and edit its content."
          />
        )}
      </div>
    </div>
  );
}

// ── Search Results ──────────────────────────────────────────────────────────

function SearchResults({
  results,
  loading,
  query,
  onBlockSelect,
}: {
  results: Array<{ block: ScopedBlock; score: number }>;
  loading: boolean;
  query: string;
  onBlockSelect: (block: ScopedBlock) => void;
}) {
  if (loading) {
    return (
      <div className="flex justify-center py-12">
        <Spinner size="md" />
      </div>
    );
  }

  if (results.length === 0) {
    return (
      <EmptyState
        icon={<Search size={24} />}
        title="No results"
        description={`No memory blocks match "${query}".`}
      />
    );
  }

  return (
    <div className="space-y-2">
      <p className="text-sm text-sera-text-muted mb-3">
        {results.length} result{results.length !== 1 ? 's' : ''} for "{query}"
      </p>
      {results.map(({ block, score }) => (
        <div key={block.id} className="flex items-start gap-2">
          <div className="flex-1">
            <BlockCard block={block} showAgent onClick={() => onBlockSelect(block)} />
          </div>
          <span className="text-[10px] text-sera-text-dim mt-2 shrink-0">
            {Math.round(score * 100)}%
          </span>
        </div>
      ))}
    </div>
  );
}

// ── Block Detail ────────────────────────────────────────────────────────────

function BlockDetail({
  block,
  loading,
  backlinks,
  onBlockSelect,
}: {
  block: ScopedBlock | null;
  loading: boolean;
  backlinks: Array<{
    sourceId: string;
    sourceTitle: string;
    sourceType: string;
    relationship: string;
  }>;
  onBlockSelect: (block: ScopedBlock) => void;
}) {
  const [editContent, setEditContent] = useState<string | null>(null);

  if (loading) {
    return (
      <div className="flex justify-center py-12">
        <Spinner size="md" />
      </div>
    );
  }

  if (!block) {
    return (
      <EmptyState
        icon={<Brain size={24} />}
        title="Block not found"
        description="The selected memory block could not be loaded."
      />
    );
  }

  const content = editContent ?? block.content;
  const isEditing = editContent !== null;

  return (
    <div className="space-y-4">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2 mb-2">
          <Badge variant="accent">{block.type}</Badge>
          <span className="text-[10px] text-sera-text-dim">{block.agentId}</span>
          <span className="text-[10px] text-sera-text-dim ml-auto">
            {new Date(block.timestamp).toLocaleString()}
          </span>
        </div>
        <h2 className="text-lg font-semibold text-sera-text">{block.title}</h2>
      </div>

      {/* Tags */}
      {block.tags.length > 0 && (
        <div className="flex gap-1 flex-wrap">
          {block.tags.map((tag) => (
            <Badge key={tag} variant="default">
              {tag}
            </Badge>
          ))}
        </div>
      )}

      {/* Content */}
      <div>
        {isEditing ? (
          <div className="space-y-2">
            <textarea
              value={content}
              onChange={(e) => setEditContent(e.target.value)}
              className="sera-input w-full min-h-[200px] font-mono text-sm"
            />
            <div className="flex gap-2">
              <Button
                size="sm"
                onClick={() => {
                  // Save would call an update API here
                  toast.success('Block updated');
                  setEditContent(null);
                }}
              >
                <Save size={12} className="mr-1" /> Save
              </Button>
              <Button size="sm" variant="outline" onClick={() => setEditContent(null)}>
                Cancel
              </Button>
            </div>
          </div>
        ) : (
          <div
            className="sera-card-static p-4 whitespace-pre-wrap text-sm text-sera-text cursor-pointer hover:border-sera-accent/30 transition-colors"
            onClick={() => setEditContent(block.content)}
            title="Click to edit"
          >
            {block.content || '(empty)'}
          </div>
        )}
      </div>

      {/* Importance */}
      <div className="flex items-center gap-1">
        <span className="text-xs text-sera-text-dim">Importance:</span>
        {[1, 2, 3, 4, 5].map((i) => (
          <span
            key={i}
            className={`text-sm ${i <= block.importance ? 'text-sera-accent' : 'text-sera-text-dim'}`}
          >
            *
          </span>
        ))}
      </div>

      {/* Backlinks */}
      {backlinks.length > 0 && (
        <div>
          <h3 className="sera-section-label flex items-center gap-1 mb-2">
            <ArrowDownLeft size={12} /> Referenced by ({backlinks.length})
          </h3>
          <div className="space-y-1">
            {backlinks.map((bl) => (
              <button
                key={bl.sourceId}
                type="button"
                onClick={() =>
                  onBlockSelect({
                    id: bl.sourceId,
                    agentId: block.agentId,
                    title: bl.sourceTitle,
                    type: bl.sourceType,
                    content: '',
                    tags: [],
                    importance: 3,
                    timestamp: '',
                  })
                }
                className="flex items-center gap-2 text-sm text-sera-text hover:text-sera-accent transition-colors w-full text-left"
              >
                <ArrowUpRight size={12} className="text-sera-text-dim" />
                <span className="truncate">{bl.sourceTitle}</span>
                <Badge variant="default" className="text-[9px]">
                  {bl.relationship}
                </Badge>
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Metadata */}
      <div className="text-[10px] text-sera-text-dim space-y-0.5 pt-2 border-t border-sera-border">
        <div>ID: {block.id}</div>
        <div>Agent: {block.agentId}</div>
      </div>
    </div>
  );
}
