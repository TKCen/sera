import { useState, useCallback } from 'react';
import { Link } from 'react-router';
import { Brain, Clock, ExternalLink, Plus } from 'lucide-react';
import { toast } from 'sonner';
import { useAgentMemory } from '@/hooks/useAgents';
import { addMemoryEntry } from '@/lib/api/memory';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { EmptyState } from '@/components/EmptyState';
import { TabLoading } from '@/components/AgentDetailTabLoading';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';

const MEMORY_TYPES = ['fact', 'preference', 'episode', 'insight', 'task', 'note'] as const;

export function MemoryTab({ id }: { id: string }) {
  const [scope, setScope] = useState<string>('');
  const { data: blocks, isLoading, refetch } = useAgentMemory(id, scope || undefined);
  const [showCreate, setShowCreate] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newEntry, setNewEntry] = useState({
    type: 'note' as string,
    title: '',
    content: '',
    tags: '',
  });

  const handleCreate = useCallback(async () => {
    if (!newEntry.title.trim() || !newEntry.content.trim()) {
      toast.error('Title and content are required');
      return;
    }
    setCreating(true);
    try {
      const tags = newEntry.tags
        .split(',')
        .map((t) => t.trim())
        .filter(Boolean);
      await addMemoryEntry(newEntry.type, {
        title: newEntry.title.trim(),
        content: newEntry.content.trim(),
        ...(tags.length > 0 ? { tags } : {}),
        source: 'manual',
      });
      toast.success('Memory entry created');
      setShowCreate(false);
      setNewEntry({ type: 'note', title: '', content: '', tags: '' });
      void refetch();
    } catch {
      toast.error('Failed to create memory entry');
    } finally {
      setCreating(false);
    }
  }, [newEntry, refetch]);

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
        <div className="flex items-center gap-3">
          <Button size="sm" variant="outline" onClick={() => setShowCreate(true)}>
            <Plus size={12} /> Add Entry
          </Button>
          <Link
            to={`/memory/${id}`}
            className="flex items-center gap-1 text-xs text-sera-accent hover:underline"
          >
            <ExternalLink size={11} /> Browse all
          </Link>
          <Link
            to={`/agents/${id}/memory-graph`}
            className="flex items-center gap-1 text-xs text-sera-accent hover:underline"
          >
            <ExternalLink size={11} /> Graph view
          </Link>
        </div>
      </div>

      {isLoading ? (
        <TabLoading />
      ) : !blocks?.length ? (
        <EmptyState
          icon={<Brain size={24} />}
          title="No memories yet"
          description="This agent hasn't formed any memories."
          action={
            <Button size="sm" onClick={() => setShowCreate(true)}>
              <Plus size={12} className="mr-1" /> Create Entry
            </Button>
          }
        />
      ) : (
        <div className="space-y-2">
          {blocks.map((block) => (
            <div key={block.id} className="sera-card flex items-start gap-3 p-3">
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
            </div>
          ))}
        </div>
      )}

      {/* Create Memory Entry Dialog */}
      <Dialog open={showCreate} onOpenChange={(o: boolean) => !o && setShowCreate(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Add Memory Entry</DialogTitle>
            <DialogDescription>Manually create a memory entry for this agent.</DialogDescription>
          </DialogHeader>
          <div className="space-y-3 mt-2">
            <div>
              <label className="block text-xs text-sera-text-muted mb-1">Type</label>
              <select
                value={newEntry.type}
                onChange={(e) => setNewEntry((s) => ({ ...s, type: e.target.value }))}
                className="sera-input text-xs w-full"
              >
                {MEMORY_TYPES.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-xs text-sera-text-muted mb-1">Title</label>
              <input
                type="text"
                value={newEntry.title}
                onChange={(e) => setNewEntry((s) => ({ ...s, title: e.target.value }))}
                placeholder="e.g. User prefers concise responses"
                className="sera-input text-xs w-full"
              />
            </div>
            <div>
              <label className="block text-xs text-sera-text-muted mb-1">Content</label>
              <textarea
                value={newEntry.content}
                onChange={(e) => setNewEntry((s) => ({ ...s, content: e.target.value }))}
                placeholder="The memory content…"
                rows={4}
                className="sera-input text-xs w-full resize-none"
              />
            </div>
            <div>
              <label className="block text-xs text-sera-text-muted mb-1">
                Tags (comma-separated, optional)
              </label>
              <input
                type="text"
                value={newEntry.tags}
                onChange={(e) => setNewEntry((s) => ({ ...s, tags: e.target.value }))}
                placeholder="e.g. preference, style"
                className="sera-input text-xs w-full"
              />
            </div>
          </div>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button size="sm" onClick={() => void handleCreate()} disabled={creating}>
              Create
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
