import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog';
import { useCreateMemoryBlock } from '@/hooks/useMemoryExplorer';

const BLOCK_TYPES = ['fact', 'preference', 'episode', 'insight', 'task', 'note'] as const;

interface MemoryBlockCreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function MemoryBlockCreateDialog({ open, onOpenChange }: MemoryBlockCreateDialogProps) {
  const createBlock = useCreateMemoryBlock();
  const [form, setForm] = useState({
    type: 'note' as string,
    title: '',
    content: '',
    tags: '',
  });

  const handleSubmit = async () => {
    if (!form.title.trim() || !form.content.trim()) {
      toast.error('Title and content are required');
      return;
    }
    try {
      await createBlock.mutateAsync({
        type: form.type,
        entry: {
          title: form.title.trim(),
          content: form.content.trim(),
          tags: form.tags
            .split(',')
            .map((t) => t.trim())
            .filter(Boolean),
        },
      });
      toast.success('Memory block created');
      setForm({ type: 'note', title: '', content: '', tags: '' });
      onOpenChange(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to create block');
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New Memory Block</DialogTitle>
          <DialogDescription>
            Create a new memory block in the global knowledge base.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4 py-4">
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted font-medium">Type</label>
            <select
              value={form.type}
              onChange={(e) => setForm({ ...form, type: e.target.value })}
              className="sera-input text-sm w-full"
            >
              {BLOCK_TYPES.map((t) => (
                <option key={t} value={t}>
                  {t.charAt(0).toUpperCase() + t.slice(1)}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted font-medium">Title</label>
            <Input
              value={form.title}
              onChange={(e) => setForm({ ...form, title: e.target.value })}
              placeholder="Block title"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted font-medium">Content</label>
            <textarea
              value={form.content}
              onChange={(e) => setForm({ ...form, content: e.target.value })}
              placeholder="Block content..."
              rows={4}
              className="sera-input text-sm w-full resize-y"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted font-medium">
              Tags (comma-separated)
            </label>
            <Input
              value={form.tags}
              onChange={(e) => setForm({ ...form, tags: e.target.value })}
              placeholder="e.g. important, project-x"
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button size="sm" onClick={handleSubmit} disabled={createBlock.isPending}>
            {createBlock.isPending && <Loader2 size={14} className="animate-spin mr-1" />}
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
