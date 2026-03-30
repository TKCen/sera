import { useState, useCallback } from 'react';
import {
  Brain,
  Plus,
  ArrowUp,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
} from 'lucide-react';
import { toast } from 'sonner';
import { MemorySidebar } from '@/components/memory/MemorySidebar';
import { MemoryContent } from '@/components/memory/MemoryContent';
import { MemoryGraphMinimap } from '@/components/memory/MemoryGraphMinimap';
import { Button } from '@/components/ui/button';
import { usePromoteBlock } from '@/hooks/useMemoryExplorer';
import { useAgents } from '@/hooks/useAgents';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import type { MemoryScope } from '@/components/memory/MemorySidebar';
import type { ScopedBlock } from '@/lib/api/memory';

function MemoryExplorerContent() {
  const [scope, setScope] = useState<MemoryScope>({ kind: 'global' });
  const [selectedBlock, setSelectedBlock] = useState<ScopedBlock | null>(null);
  const [tagFilter, setTagFilter] = useState('');
  const [searchQuery, setSearchQuery] = useState('');
  const [leftCollapsed, setLeftCollapsed] = useState(false);
  const [rightCollapsed, setRightCollapsed] = useState(false);
  const { data: agents } = useAgents();

  // Build agent UUID → display name lookup
  const agentNameMap = new Map<string, string>();
  if (agents) {
    for (const a of agents) {
      agentNameMap.set(a.id, a.name);
    }
  }
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
        {/* Left: Sidebar (collapsible) */}
        <div
          className={`border-r border-sera-border shrink-0 overflow-hidden transition-all duration-200 ${
            leftCollapsed ? 'w-10' : 'w-80 lg:w-96'
          }`}
        >
          {leftCollapsed ? (
            <button
              type="button"
              onClick={() => setLeftCollapsed(false)}
              className="w-full h-full flex items-center justify-center text-sera-text-muted hover:text-sera-text hover:bg-sera-surface/50 transition-colors"
              title="Expand sidebar"
            >
              <PanelLeftOpen size={16} />
            </button>
          ) : (
            <div className="flex flex-col h-full">
              <div className="flex items-center justify-end p-1 border-b border-sera-border">
                <button
                  type="button"
                  onClick={() => setLeftCollapsed(true)}
                  className="p-1 text-sera-text-dim hover:text-sera-text transition-colors"
                  title="Collapse sidebar"
                >
                  <PanelLeftClose size={14} />
                </button>
              </div>
              <div className="flex-1 overflow-hidden">
                <MemorySidebar
                  scope={scope}
                  onScopeChange={setScope}
                  selectedBlockId={selectedBlock?.id ?? null}
                  onBlockSelect={handleBlockSelect}
                  tagFilter={tagFilter}
                  onTagFilter={setTagFilter}
                  agentNameMap={agentNameMap}
                />
              </div>
            </div>
          )}
        </div>

        {/* Center: Content */}
        <div className="flex-1 overflow-hidden min-w-0">
          <MemoryContent
            selectedAgentId={selectedBlock?.agentId ?? ''}
            selectedBlockId={selectedBlock?.id ?? ''}
            onBlockSelect={handleBlockSelect}
            onSearchChange={setSearchQuery}
            agentNameMap={agentNameMap}
          />
        </div>

        {/* Right: Graph minimap (collapsible) */}
        <div
          className={`border-l border-sera-border shrink-0 overflow-hidden transition-all duration-200 ${
            rightCollapsed ? 'w-10' : 'w-80 lg:w-[28rem]'
          }`}
        >
          {rightCollapsed ? (
            <button
              type="button"
              onClick={() => setRightCollapsed(false)}
              className="w-full h-full flex items-center justify-center text-sera-text-muted hover:text-sera-text hover:bg-sera-surface/50 transition-colors"
              title="Expand graph"
            >
              <PanelRightOpen size={16} />
            </button>
          ) : (
            <div className="flex flex-col h-full">
              <div className="flex items-center justify-start p-1 border-b border-sera-border">
                <button
                  type="button"
                  onClick={() => setRightCollapsed(true)}
                  className="p-1 text-sera-text-dim hover:text-sera-text transition-colors"
                  title="Collapse graph"
                >
                  <PanelRightClose size={14} />
                </button>
              </div>
              <div className="flex-1 overflow-hidden">
                <MemoryGraphMinimap
                  onNodeSelect={handleBlockSelect}
                  selectedBlockId={selectedBlock?.id ?? null}
                  searchQuery={searchQuery}
                  tagFilter={tagFilter}
                  scopeAgentId={scope.kind === 'agent' ? scope.agentId : undefined}
                />
              </div>
            </div>
          )}
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
