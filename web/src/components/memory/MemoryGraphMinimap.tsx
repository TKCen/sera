import { Suspense, lazy, useState } from 'react';
import { Maximize2, Minimize2 } from 'lucide-react';
import { useExplorerGraph } from '@/hooks/useMemoryExplorer';
import { Spinner } from '@/components/ui/spinner';
import type { ScopedBlock } from '@/lib/api/memory';

const MemoryGraph = lazy(() => import('@/components/MemoryGraph'));

interface MemoryGraphMinimapProps {
  onNodeSelect: (block: ScopedBlock) => void;
  selectedBlockId: string | null;
  searchQuery?: string;
  tagFilter?: string;
  scopeAgentId?: string;
}

export function MemoryGraphMinimap({
  onNodeSelect,
  selectedBlockId,
  searchQuery,
  tagFilter,
  scopeAgentId,
}: MemoryGraphMinimapProps) {
  const [expanded, setExpanded] = useState(false);
  const { data: graphData, isLoading } = useExplorerGraph();

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Spinner size="sm" />
      </div>
    );
  }

  if (!graphData || graphData.nodes.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-sera-text-dim text-xs">
        No graph data
      </div>
    );
  }

  // Filter nodes by scope if an agent is selected
  const filteredNodes = scopeAgentId
    ? graphData.nodes.filter(
        (n) => n.nodeKind === 'agent' || n.nodeKind === 'circle' || n.agentId === scopeAgentId
      )
    : graphData.nodes;

  const filteredNodeIds = new Set(filteredNodes.map((n) => n.id));
  const filteredEdges = graphData.edges.filter(
    (e) => filteredNodeIds.has(e.source) && filteredNodeIds.has(e.target)
  );

  // Transform to MemoryGraph format
  const memoryGraphData = {
    nodes: filteredNodes.map((n) => ({
      id: n.id,
      title: n.title,
      type: n.nodeKind === 'agent' ? 'agent' : n.nodeKind === 'circle' ? 'circle' : n.type,
      tags: n.tags,
    })),
    edges: filteredEdges.map((e) => ({
      from: e.source,
      to: e.target,
      kind: (e.kind === 'wikilink' ? 'wikilink' : 'ref') as 'ref' | 'wikilink',
    })),
  };

  // Build a combined highlight query from search, tag filter, and selection
  const highlightQuery = searchQuery || tagFilter || selectedBlockId || '';

  const handleNodeClick = (node: { id: string; title: string; type: string; tags: string[] }) => {
    // Don't select agent/circle meta-nodes
    if (node.id.startsWith('agent:') || node.id.startsWith('circle:')) return;

    const graphNode = graphData.nodes.find((n) => n.id === node.id);
    if (!graphNode || graphNode.nodeKind !== 'block') return;

    onNodeSelect({
      id: node.id,
      agentId: graphNode.agentId ?? '',
      title: node.title,
      type: node.type,
      content: '',
      tags: node.tags,
      importance: 3,
      timestamp: '',
    });
  };

  return (
    <div
      className={`relative ${
        expanded ? 'fixed inset-0 z-50 bg-sera-bg/95 backdrop-blur-sm' : 'h-full'
      }`}
    >
      {/* Expand/collapse button */}
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="absolute top-2 right-2 z-10 p-1.5 rounded bg-sera-surface/80 hover:bg-sera-surface text-sera-text-muted hover:text-sera-text transition-colors"
        title={expanded ? 'Collapse' : 'Expand graph'}
      >
        {expanded ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
      </button>

      <Suspense
        fallback={
          <div className="flex items-center justify-center h-full">
            <Spinner size="sm" />
          </div>
        }
      >
        <MemoryGraph
          data={memoryGraphData}
          onNodeClick={handleNodeClick}
          searchQuery={highlightQuery || undefined}
          className="w-full h-full"
        />
      </Suspense>
    </div>
  );
}
