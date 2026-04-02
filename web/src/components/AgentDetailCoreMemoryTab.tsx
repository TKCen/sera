import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Brain, Save, Lock, Unlock, AlertCircle } from 'lucide-react';
import { toast } from 'sonner';
import { getCoreMemoryBlocks, updateCoreMemoryBlock, type CoreMemoryBlock } from '@/lib/api/memory';
import { Button } from '@/components/ui/button';
import { TabLoading } from '@/components/AgentDetailTabLoading';
import { EmptyState } from '@/components/EmptyState';
import { Progress } from '@/components/ui/progress';

export function CoreMemoryTab({ id }: { id: string }) {
  const queryClient = useQueryClient();
  const { data: blocks, isLoading } = useQuery({
    queryKey: ['agent-core-memory', id],
    queryFn: () => getCoreMemoryBlocks(id),
  });

  const updateMutation = useMutation({
    mutationFn: ({
      name,
      updates,
    }: {
      name: string;
      updates: { content?: string; characterLimit?: number; isReadOnly?: boolean };
    }) => updateCoreMemoryBlock(id, name, updates),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['agent-core-memory', id] });
      toast.success('Core memory block updated');
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : 'Failed to update block');
    },
  });

  if (isLoading) return <TabLoading />;

  if (!blocks || blocks.length === 0) {
    return (
      <EmptyState
        icon={<Brain size={24} />}
        title="No core memory"
        description="This agent has no core memory blocks initialized."
      />
    );
  }

  return (
    <div className="p-6 space-y-6 max-w-4xl">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-semibold text-sera-text font-mono uppercase tracking-wider">
            Core Memory Blocks
          </h3>
          <p className="text-xs text-sera-text-muted mt-1">
            Static memory blocks injected directly into the system prompt.
          </p>
        </div>
      </div>

      <div className="grid gap-6">
        {blocks.map((block) => (
          <BlockEditor
            key={block.id}
            block={block}
            onSave={(content) => updateMutation.mutate({ name: block.name, updates: { content } })}
            isUpdating={updateMutation.isPending && updateMutation.variables?.name === block.name}
          />
        ))}
      </div>
    </div>
  );
}

function BlockEditor({
  block,
  onSave,
  isUpdating,
}: {
  block: CoreMemoryBlock;
  onSave: (content: string) => void;
  isUpdating: boolean;
}) {
  const [content, setContent] = useState(block.content);
  const charCount = content.length;
  const isOverLimit = charCount > block.characterLimit;
  const hasChanged = content !== block.content;

  return (
    <div className="sera-card-static p-4 space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-xs font-bold text-sera-text uppercase tracking-tight">
            {block.name}
          </span>
          {block.isReadOnly ? (
            <Lock size={12} className="text-sera-text-dim" title="Read Only" />
          ) : (
            <Unlock size={12} className="text-sera-accent" title="Editable by Agent" />
          )}
        </div>
        <div className="flex items-center gap-3">
          <div className="flex flex-col items-end gap-1 min-w-[100px]">
            <div className="flex items-center gap-1.5 text-[10px] font-medium">
              <span className={isOverLimit ? 'text-sera-error' : 'text-sera-text-muted'}>
                {charCount.toLocaleString()}
              </span>
              <span className="text-sera-text-dim">/</span>
              <span className="text-sera-text-dim">{block.characterLimit.toLocaleString()}</span>
            </div>
            <Progress
              value={(charCount / block.characterLimit) * 100}
              className="h-1 w-24"
              variant={isOverLimit ? 'danger' : 'default'}
            />
          </div>
          <Button
            size="xs"
            variant="ghost"
            onClick={() => onSave(content)}
            disabled={!hasChanged || isOverLimit || isUpdating}
          >
            {isUpdating ? (
              'Saving...'
            ) : (
              <>
                <Save size={12} className="mr-1" /> Save
              </>
            )}
          </Button>
        </div>
      </div>

      <textarea
        value={content}
        onChange={(e) => setContent(e.target.value)}
        className="w-full bg-sera-surface border border-sera-border rounded p-3 text-xs font-mono min-h-[120px] focus:ring-1 focus:ring-sera-accent outline-none transition-shadow"
        placeholder={`Enter ${block.name} content...`}
      />

      {isOverLimit && (
        <div className="flex items-center gap-1.5 text-sera-error text-[10px] font-medium">
          <AlertCircle size={10} />
          Content exceeds character limit of {block.characterLimit.toLocaleString()}
        </div>
      )}
    </div>
  );
}
