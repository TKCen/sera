import { useState, useEffect } from 'react';
import { toast } from 'sonner';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useCreateSkill } from '@/hooks/useSkills';
import { Eye, Code } from 'lucide-react';
import { cn } from '@/lib/utils';

interface SkillEditorDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Pre-populate for editing an existing skill */
  initial?: {
    name: string;
    version: string;
    description: string;
    triggers: string[];
    category?: string;
    tags?: string[];
    maxTokens?: number;
    content: string;
  };
}

export function SkillEditorDialog({ open, onOpenChange, initial }: SkillEditorDialogProps) {
  const createSkill = useCreateSkill();
  const [preview, setPreview] = useState(false);

  const [name, setName] = useState('');
  const [version, setVersion] = useState('1.0.0');
  const [description, setDescription] = useState('');
  const [triggers, setTriggers] = useState('');
  const [category, setCategory] = useState('');
  const [tags, setTags] = useState('');
  const [maxTokens, setMaxTokens] = useState<number | ''>('');
  const [content, setContent] = useState('');

  useEffect(() => {
    if (initial) {
      setName(initial.name);
      setVersion(initial.version);
      setDescription(initial.description);
      setTriggers(initial.triggers.join(', '));
      setCategory(initial.category ?? '');
      setTags(initial.tags?.join(', ') ?? '');
      setMaxTokens(initial.maxTokens ?? '');
      setContent(initial.content);
    } else {
      setName('');
      setVersion('1.0.0');
      setDescription('');
      setTriggers('');
      setCategory('');
      setTags('');
      setMaxTokens('');
      setContent('');
    }
    setPreview(false);
  }, [initial, open]);

  async function handleSave() {
    if (!name || !description || !content) {
      toast.error('Name, description, and content are required');
      return;
    }

    const parsedTriggers = triggers
      .split(',')
      .map((t) => t.trim())
      .filter(Boolean);
    const parsedTags = tags
      .split(',')
      .map((t) => t.trim())
      .filter(Boolean);

    try {
      await createSkill.mutateAsync({
        name,
        version,
        description,
        triggers: parsedTriggers.length > 0 ? parsedTriggers : [name.toLowerCase()],
        content,
        ...(category ? { category } : {}),
        ...(parsedTags.length > 0 ? { tags: parsedTags } : {}),
        ...(maxTokens !== '' ? { maxTokens: Number(maxTokens) } : {}),
      });
      toast.success(`Skill "${name}" saved`);
      onOpenChange(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to save skill');
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{initial ? 'Edit Skill' : 'Create Guidance Skill'}</DialogTitle>
          <DialogDescription>
            Guidance skills are markdown documents injected into agent context to shape behavior.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {/* Metadata fields */}
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">Name *</label>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="my-skill"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">
                Version *
              </label>
              <Input
                value={version}
                onChange={(e) => setVersion(e.target.value)}
                placeholder="1.0.0"
              />
            </div>
          </div>

          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1">
              Description *
            </label>
            <Input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="What this skill teaches agents to do"
            />
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">
                Triggers
              </label>
              <Input
                value={triggers}
                onChange={(e) => setTriggers(e.target.value)}
                placeholder="typescript, coding"
              />
              <p className="text-[9px] text-sera-text-dim mt-0.5">Comma-separated keywords</p>
            </div>
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">
                Category
              </label>
              <Input
                value={category}
                onChange={(e) => setCategory(e.target.value)}
                placeholder="engineering/typescript"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">Tags</label>
              <Input
                value={tags}
                onChange={(e) => setTags(e.target.value)}
                placeholder="best-practices, patterns"
              />
            </div>
          </div>

          {/* Content editor */}
          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="text-xs font-medium text-sera-text-muted">
                Skill Content (Markdown) *
              </label>
              <button
                onClick={() => setPreview((p) => !p)}
                className="text-[10px] text-sera-text-dim hover:text-sera-text flex items-center gap-1 transition-colors"
              >
                {preview ? <Code size={10} /> : <Eye size={10} />}
                {preview ? 'Edit' : 'Preview'}
              </button>
            </div>

            {preview ? (
              <div className="sera-card-static p-4 min-h-[200px] max-h-[400px] overflow-y-auto prose prose-invert prose-xs max-w-none">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
              </div>
            ) : (
              <textarea
                value={content}
                onChange={(e) => setContent(e.target.value)}
                placeholder="# Skill Title&#10;&#10;Write guidance for agents here...&#10;&#10;## Section&#10;- Point 1&#10;- Point 2"
                className={cn(
                  'w-full h-64 bg-sera-bg border border-sera-border rounded-lg p-3',
                  'text-xs font-mono text-sera-text resize-y',
                  'outline-none focus:border-sera-accent',
                  'placeholder:text-sera-text-dim'
                )}
              />
            )}
          </div>

          {/* Actions */}
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={() => {
                void handleSave();
              }}
              disabled={createSkill.isPending || !name || !description || !content}
            >
              {createSkill.isPending ? 'Saving…' : initial ? 'Update Skill' : 'Create Skill'}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
