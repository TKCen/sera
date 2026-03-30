import { useState, useCallback } from 'react';
import { Brain, Plus, ArrowUp } from 'lucide-react';
import { toast } from 'sonner';
import { MemorySidebar } from '@/components/memory/MemorySidebar';
import { MemoryContent } from '@/components/memory/MemoryContent';
import { MemoryGraphMinimap } from '@/components/memory/MemoryGraphMinimap';
import { Button } from '@/components/ui/button';
import { usePromoteBlock } from '@/hooks/useMemoryExplorer';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import type { MemoryScope } from '@/components/memory/MemorySidebar';
import type { ScopedBlock } from '@/lib/api/memory';

function MemoryExplorerContent() {
  const [scope, setScope] = useState<MemoryScope>({ kind: 'global' });
  const [selectedBlock, setSelectedBlock] = useState<ScopedBlock | null>(null);
  const [tagFilter, setTagFilter] = useState('');
  const promoteMutation = usePromoteBlock();

  const handleBlockSelect = useCallback((block: ScopedBlock) => {
    setSelectedBlock(block);
  }, []);

  const handlePromote = useCallback(
    async (targetScope: 'circle' | 'global') => {
      if (!selectedBlock) return;
      try {
        await promoteMutation.mutateAsync({
          agentId: selectedBlock.agentId,
          blockId: selectedBlock.id,
          targetScope,
        });
        toast.success(`Block promoted to ${targetScope}`);
      } catch (err) {
        toast.error(`Promotion failed: ${err instanceof Error ? err.message : String(err)}`);
      }
    },
    [selectedBlock, promoteMutation]
  );

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Page header */}
      <div className="flex items-center justify-between p-4 border-b border-sera-border shrink-0">
        <h1 className="sera-page-title flex items-center gap-2">
          <Brain size={20} /> Memory Explorer
        </h1>
        <div className="flex items-center gap-2">
          {selectedBlock && (
            <>
              <Button
                size="sm"
                variant="outline"
                onClick={() => handlePromote('circle')}
                disabled={promoteMutation.isPending}
              >
                <ArrowUp size={12} className="mr-1" /> Promote to Circle
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => handlePromote('global')}
                disabled={promoteMutation.isPending}
              >
                <ArrowUp size={12} className="mr-1" /> Promote to Global
              </Button>
            </>
          )}
          <Button size="sm">
            <Plus size={12} className="mr-1" /> New Block
          </Button>
        </div>
      </div>

      {/* Three-panel layout */}
      <div className="flex flex-1 overflow-hidden">
        {/* Left: Sidebar */}
        <div className="w-72 border-r border-sera-border shrink-0 overflow-hidden">
          <MemorySidebar
            scope={scope}
            onScopeChange={setScope}
            selectedBlockId={selectedBlock?.id ?? null}
            onBlockSelect={handleBlockSelect}
            tagFilter={tagFilter}
            onTagFilter={setTagFilter}
          />
        </div>

        {/* Center: Content */}
        <div className="flex-1 overflow-hidden">
          <MemoryContent
            selectedAgentId={selectedBlock?.agentId ?? ''}
            selectedBlockId={selectedBlock?.id ?? ''}
            onBlockSelect={handleBlockSelect}
          />
        </div>

        {/* Right: Graph minimap */}
        <div className="w-80 border-l border-sera-border shrink-0 overflow-hidden">
          <MemoryGraphMinimap
            onNodeSelect={handleBlockSelect}
            selectedBlockId={selectedBlock?.id ?? null}
          />
        </div>
      </div>
    </div>
  );
}

export default function MemoryExplorerPage() {
  return (
    <ErrorBoundary>
      <MemoryExplorerContent />
    </ErrorBoundary>
  );
}
