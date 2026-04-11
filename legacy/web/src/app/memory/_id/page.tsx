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
  Pencil,
  Trash2,
  Loader2,
} from 'lucide-react';
import { toast } from 'sonner';
import { getAgentBlocks, getAgentStats, getAgentLinks } from '@/lib/api/memory';
import type { ScopedBlock } from '@/lib/api/memory';
import { useUpdateBlock, useDeleteBlock } from '@/hooks/useMemoryExplorer';
import { Spinner } from '@/components/ui/spinner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from '@/components/ui/dialog';
import { cn } from '@/lib/utils';
import { MEMORY_TYPE_TAILWIND } from '@/components/memory/constants';

function BlockCard({ block, agentId }: { block: ScopedBlock; agentId: string }) {
  const [expanded, setExpanded] = useState(false);
  const [showEdit, setShowEdit] = useState(false);
  const [showDelete, setShowDelete] = useState(false);
  const [editForm, setEditForm] = useState({
    title: block.title,
    content: block.content,
    tags: block.tags.join(', '),
    importance: block.importance,
  });

  const updateBlock = useUpdateBlock();
  const deleteBlock = useDeleteBlock();

  const { data: links } = useQuery({
    queryKey: ['memory-links', agentId, block.id],
    queryFn: () => getAgentLinks(agentId, block.id),
    enabled: expanded,
  });

  const handleEdit = async () => {
    try {
      await updateBlock.mutateAsync({
        agentId,
        blockId: block.id,
        updates: {
          title: editForm.title.trim(),
          content: editForm.content.trim(),
          tags: editForm.tags
            .split(',')
            .map((t) => t.trim())
            .filter(Boolean),
          importance: editForm.importance,
        },
      });
      toast.success('Block updated');
      setShowEdit(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to update block');
    }
  };

  const handleDelete = async () => {
    try {
      await deleteBlock.mutateAsync({ agentId, blockId: block.id });
      toast.success('Block deleted');
      setShowDelete(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to delete block');
    }
  };

  return (
    <>
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
              <span className="ml-auto flex items-center gap-1.5 flex-shrink-0">
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setShowEdit(true);
                  }}
                  className="p-1 rounded text-sera-text-dim hover:text-sera-text hover:bg-sera-surface-hover transition-colors"
                  title="Edit block"
                >
                  <Pencil size={12} />
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setShowDelete(true);
                  }}
                  className="p-1 rounded text-sera-text-dim hover:text-sera-error hover:bg-sera-error/10 transition-colors"
                  title="Delete block"
                >
                  <Trash2 size={12} />
                </button>
                <span className="text-[10px] text-sera-text-dim">
                  {new Date(block.timestamp).toLocaleDateString()}
                </span>
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

      {/* Edit Dialog */}
      <Dialog open={showEdit} onOpenChange={setShowEdit}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Edit Memory Block</DialogTitle>
            <DialogDescription>
              Update the block&apos;s title, content, tags, or importance.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-1.5">
              <label className="text-xs text-sera-text-muted font-medium">Title</label>
              <Input
                value={editForm.title}
                onChange={(e) => setEditForm({ ...editForm, title: e.target.value })}
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs text-sera-text-muted font-medium">Content</label>
              <textarea
                value={editForm.content}
                onChange={(e) => setEditForm({ ...editForm, content: e.target.value })}
                rows={6}
                className="sera-input text-sm w-full resize-y font-mono"
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs text-sera-text-muted font-medium">
                Tags (comma-separated)
              </label>
              <Input
                value={editForm.tags}
                onChange={(e) => setEditForm({ ...editForm, tags: e.target.value })}
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs text-sera-text-muted font-medium">Importance (1-5)</label>
              <Input
                type="number"
                min={1}
                max={5}
                value={editForm.importance}
                onChange={(e) => setEditForm({ ...editForm, importance: Number(e.target.value) })}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" size="sm" onClick={() => setShowEdit(false)}>
              Cancel
            </Button>
            <Button size="sm" onClick={handleEdit} disabled={updateBlock.isPending}>
              {updateBlock.isPending && <Loader2 size={14} className="animate-spin mr-1" />}
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation Dialog */}
      <Dialog open={showDelete} onOpenChange={setShowDelete}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Memory Block</DialogTitle>
            <DialogDescription>
              This will permanently delete &quot;{block.title || 'Untitled'}&quot;. This cannot be
              undone.
            </DialogDescription>
          </DialogHeader>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button
              size="sm"
              variant="danger"
              onClick={handleDelete}
              disabled={deleteBlock.isPending}
            >
              {deleteBlock.isPending && <Loader2 size={14} className="animate-spin mr-1" />}
              Delete
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </>
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

      {/* Blocks */}
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
