import { useState } from 'react';
import { useParams, Link } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import {
  ArrowLeft,
  Search,
  Tag,
  FileText,
  Brain,
  Link2,
  ChevronDown,
  ChevronRight,
  Edit2,
  Save,
  X,
  Lock,
} from 'lucide-react';
import {
  getAgentBlocks,
  getAgentStats,
  getAgentLinks,
  getCoreMemory,
  updateCoreMemory,
} from '@/lib/api/memory';
import type { ScopedBlock, CoreMemoryBlock } from '@/lib/api/memory';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { Spinner } from '@/components/ui/spinner';
import { cn } from '@/lib/utils';
import { MEMORY_TYPE_TAILWIND } from '@/components/memory/constants';

function CoreMemoryCard({ block, agentId }: { block: CoreMemoryBlock; agentId: string }) {
  const [isEditing, setIsEditing] = useState(false);
  const [content, setContent] = useState(block.content);
  const queryClient = useQueryClient();

  const updateMutation = useMutation({
    mutationFn: (newContent: string) => updateCoreMemory(agentId, block.name, newContent),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['core-memory', agentId] });
      setIsEditing(false);
      toast.success(`Core memory "${block.name}" updated`);
    },
    onError: (err: Error) => {
      toast.error(`Failed to update core memory: ${err.message}`);
    },
  });

  const handleSave = () => {
    updateMutation.mutate(content);
  };

  const handleCancel = () => {
    setContent(block.content);
    setIsEditing(false);
  };

  return (
    <div className="sera-card-static p-4 border-l-4 border-l-sera-accent">
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2">
          <span className="text-sm font-bold text-sera-text uppercase tracking-tight">
            {block.name}
          </span>
          {block.isReadonly && <Lock size={12} className="text-sera-text-dim" />}
          <span className="text-[10px] text-sera-text-dim font-mono">
            {content.length} / {block.charLimit} chars
          </span>
        </div>
        {!block.isReadonly && !isEditing && (
          <button
            onClick={() => setIsEditing(true)}
            className="p-1 hover:bg-sera-surface rounded text-sera-text-muted hover:text-sera-accent transition-colors"
          >
            <Edit2 size={14} />
          </button>
        )}
        {isEditing && (
          <div className="flex items-center gap-2">
            <button
              onClick={handleSave}
              disabled={updateMutation.isPending}
              className="p-1 hover:bg-sera-surface rounded text-green-500 hover:text-green-400 transition-colors"
            >
              <Save size={14} />
            </button>
            <button
              onClick={handleCancel}
              className="p-1 hover:bg-sera-surface rounded text-red-500 hover:text-red-400 transition-colors"
            >
              <X size={14} />
            </button>
          </div>
        )}
      </div>

      {isEditing ? (
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          className="w-full h-32 sera-input text-xs font-mono p-3 leading-relaxed"
          placeholder={`Enter ${block.name} content…`}
        />
      ) : (
        <pre className="text-xs text-sera-text leading-relaxed whitespace-pre-wrap font-mono bg-sera-bg/50 rounded-lg p-3">
          {block.content || <span className="italic text-sera-text-dim">(empty)</span>}
        </pre>
      )}
    </div>
  );
}

function BlockCard({ block, agentId }: { block: ScopedBlock; agentId: string }) {
  const [expanded, setExpanded] = useState(false);

  const { data: links } = useQuery({
    queryKey: ['memory-links', agentId, block.id],
    queryFn: () => getAgentLinks(agentId, block.id),
    enabled: expanded,
  });

  return (
    <div className="sera-card-static p-4">
      <div className="flex items-start gap-3">
        <button
          onClick={() => setExpanded((e) => !e)}
          className="mt-0.5 text-sera-text-muted hover:text-sera-text transition-colors"
        >
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </button>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span
              className={cn(
                'px-1.5 py-0.5 rounded text-[10px] font-medium',
                MEMORY_TYPE_TAILWIND[block.type] ?? 'bg-sera-surface text-sera-text-muted'
              )}
            >
              {block.type}
            </span>
            <span className="text-sm font-medium text-sera-text truncate">
              {block.title || 'Untitled'}
            </span>
            <span className="ml-auto text-[10px] text-sera-text-dim flex-shrink-0">
              {new Date(block.timestamp).toLocaleDateString()}
            </span>
          </div>
          <div className="flex items-center gap-1.5 flex-wrap">
            {block.tags.map((tag) => (
              <span
                key={tag}
                className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface text-sera-text-muted"
              >
                {tag}
              </span>
            ))}
            {block.importance >= 4 && (
              <span className="text-[10px] px-1.5 py-0.5 rounded bg-yellow-500/20 text-yellow-400">
                importance: {block.importance}
              </span>
            )}
          </div>

          {expanded && (
            <div className="mt-3 space-y-3">
              <pre className="text-xs text-sera-text leading-relaxed whitespace-pre-wrap font-mono bg-sera-bg/50 rounded-lg p-3 max-h-[400px] overflow-y-auto">
                {block.content}
              </pre>
              {links && links.length > 0 && (
                <div className="space-y-1">
                  <span className="text-[10px] text-sera-text-dim uppercase tracking-wider flex items-center gap-1">
                    <Link2 size={10} /> Links
                  </span>
                  {links.map((l, i) => (
                    <div key={i} className="text-xs text-sera-text-muted flex items-center gap-2">
                      <span className="text-sera-accent font-mono">{l.relationship}</span>
                      <span>→</span>
                      <span className="font-mono text-sera-text">{l.target.slice(0, 8)}…</span>
                    </div>
                  ))}
                </div>
              )}
              <div className="text-[10px] text-sera-text-dim font-mono">ID: {block.id}</div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default function MemoryDetailPage() {
  const { id: agentId = '' } = useParams<{ id: string }>();
  const [typeFilter, setTypeFilter] = useState('');
  const [tagSearch, setTagSearch] = useState('');

  const { data: blocks, isLoading } = useQuery({
    queryKey: ['memory-blocks', agentId, typeFilter, tagSearch],
    queryFn: () =>
      getAgentBlocks(agentId, {
        ...(typeFilter ? { type: typeFilter } : {}),
        ...(tagSearch ? { tags: tagSearch } : {}),
      }),
    enabled: !!agentId,
  });

  const { data: stats } = useQuery({
    queryKey: ['memory-stats', agentId],
    queryFn: () => getAgentStats(agentId),
    enabled: !!agentId,
  });

  const { data: coreMemory, isLoading: isCoreLoading } = useQuery({
    queryKey: ['core-memory', agentId],
    queryFn: () => getCoreMemory(agentId),
    enabled: !!agentId,
  });

  const allTypes = [...new Set((blocks ?? []).map((b) => b.type))].sort();
  const allTags = [...new Set((blocks ?? []).flatMap((b) => b.tags))].sort();

  return (
    <div className="p-8 max-w-5xl mx-auto space-y-6">
      <div className="flex items-center gap-4">
        <Link
          to={`/agents/${agentId}`}
          className="text-sera-text-muted hover:text-sera-text transition-colors"
        >
          <ArrowLeft size={16} />
        </Link>
        <div>
          <h1 className="sera-page-title flex items-center gap-2">
            <Brain size={20} /> Agent Memory
          </h1>
          <p className="text-sm text-sera-text-muted mt-0.5 font-mono">{agentId}</p>
        </div>
      </div>

      {/* Stats + Graph link */}
      <div className="flex items-center gap-6 text-xs text-sera-text-muted">
        {stats && (
          <>
            <span className="flex items-center gap-1.5">
              <FileText size={12} /> {stats.blockCount} blocks
            </span>
            <span className="flex items-center gap-1.5">
              <Search size={12} /> {stats.vectorCount} vectors indexed
            </span>
          </>
        )}
        <Link
          to={`/agents/${agentId}/memory-graph`}
          className="flex items-center gap-1.5 text-sera-accent hover:underline ml-auto"
        >
          <Link2 size={12} /> Graph view
        </Link>
      </div>

      {/* Filters */}
      <div className="flex items-center gap-3 flex-wrap">
        <div className="flex items-center gap-1 border border-sera-border rounded-lg p-0.5">
          <button
            onClick={() => setTypeFilter('')}
            className={cn(
              'px-2 py-1 rounded-md text-[11px] font-medium transition-colors',
              !typeFilter
                ? 'bg-sera-accent-soft text-sera-accent'
                : 'text-sera-text-muted hover:text-sera-text'
            )}
          >
            All
          </button>
          {allTypes.map((t) => (
            <button
              key={t}
              onClick={() => setTypeFilter(typeFilter === t ? '' : t)}
              className={cn(
                'px-2 py-1 rounded-md text-[11px] font-medium transition-colors',
                typeFilter === t
                  ? 'bg-sera-accent-soft text-sera-accent'
                  : 'text-sera-text-muted hover:text-sera-text'
              )}
            >
              {t}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-1">
          <Tag size={12} className="text-sera-text-muted" />
          <input
            type="text"
            placeholder="Filter by tag…"
            value={tagSearch}
            onChange={(e) => setTagSearch(e.target.value)}
            className="sera-input text-xs min-w-[120px] max-w-[200px]"
          />
        </div>
        {allTags.length > 0 && (
          <div className="flex items-center gap-1 flex-wrap">
            {allTags.slice(0, 10).map((tag) => (
              <button
                key={tag}
                onClick={() => setTagSearch(tagSearch === tag ? '' : tag)}
                className={cn(
                  'text-[10px] px-1.5 py-0.5 rounded border transition-colors',
                  tagSearch === tag
                    ? 'bg-sera-accent-soft border-sera-accent text-sera-accent'
                    : 'border-sera-border text-sera-text-muted hover:text-sera-text'
                )}
              >
                {tag}
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Core Memory Blocks */}
      <div className="space-y-3">
        <h2 className="text-xs font-bold text-sera-text-dim uppercase tracking-widest flex items-center gap-2">
          <Brain size={14} /> Core Memory Blocks
        </h2>
        {isCoreLoading ? (
          <Spinner size="sm" />
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {(coreMemory ?? []).map((block) => (
              <CoreMemoryCard key={block.id} block={block} agentId={agentId} />
            ))}
          </div>
        )}
      </div>

      {/* Blocks */}
      <div className="space-y-3 pt-4 border-t border-sera-border">
        <h2 className="text-xs font-bold text-sera-text-dim uppercase tracking-widest flex items-center gap-2">
          <FileText size={14} /> Archival Memory Blocks
        </h2>
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center py-12">
          <Spinner />
        </div>
      ) : (blocks ?? []).length === 0 ? (
        <div className="text-center py-12">
          <Brain size={32} className="text-sera-text-dim mx-auto mb-3" />
          <p className="text-sm text-sera-text-muted">No memory blocks found.</p>
          <p className="text-xs text-sera-text-dim mt-1">
            Chat with the agent and ask it to remember something.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          <p className="text-xs text-sera-text-dim">
            {blocks!.length} block{blocks!.length !== 1 ? 's' : ''}
          </p>
          {blocks!.map((block) => (
            <BlockCard key={block.id} block={block} agentId={agentId} />
          ))}
        </div>
      )}
    </div>
  );
}
