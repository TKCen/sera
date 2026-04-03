import { useState, useMemo, Suspense, lazy } from 'react';
import { useParams, Link, useNavigate } from 'react-router';
import { ArrowLeft, X, Clock, Tag } from 'lucide-react';
import { useAgentMemory } from '@/hooks/useAgents';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import type { AgentMemoryBlock } from '@/lib/api/types';
import type { GraphNode, GraphEdge, MemoryGraphData } from '@/components/MemoryGraph';

const MemoryGraph = lazy(() => import('@/components/MemoryGraph'));

type Scope = '' | 'personal' | 'circle' | 'global';

function buildGraphData(blocks: AgentMemoryBlock[]): MemoryGraphData {
  const nodes: GraphNode[] = blocks.map((b) => ({
    id: b.id,
    title: b.title,
    type: b.type,
    tags: b.tags ?? [],
  }));

  const edges: GraphEdge[] = [];
  for (let i = 0; i < blocks.length; i++) {
    for (let j = i + 1; j < blocks.length; j++) {
      const tagsA = blocks[i].tags ?? [];
      const tagsB = blocks[j].tags ?? [];
      const shared = tagsA.some((t) => tagsB.includes(t));
      if (shared) {
        edges.push({ from: blocks[i].id, to: blocks[j].id, kind: 'ref' });
      }
    }
  }
  return { nodes, edges };
}

export default function AgentMemoryGraphPage() {
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [scope, setScope] = useState<Scope>('');
  const [selectedBlock, setSelectedBlock] = useState<AgentMemoryBlock | null>(null);

  const { data: blocks, isLoading } = useAgentMemory(id, scope || undefined);

  const graphData = useMemo(() => buildGraphData(blocks ?? []), [blocks]);

  function handleNodeClick(node: GraphNode) {
    const block = (blocks ?? []).find((b) => b.id === node.id) ?? null;
    setSelectedBlock(block);
  }

  function handleNodeDoubleClick(node: GraphNode) {
    void navigate(`/memory/${node.id}`);
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-6 pt-6 pb-4 border-b border-sera-border flex-shrink-0">
        <Link
          to={`/agents/${id}`}
          className="inline-flex items-center gap-1.5 text-xs text-sera-text-muted hover:text-sera-text mb-4 transition-colors"
        >
          <ArrowLeft size={12} /> {id}
        </Link>
        <div className="flex items-center justify-between">
          <h1 className="sera-page-title">Memory Graph</h1>
          <div className="flex gap-1">
            {(['', 'personal', 'circle', 'global'] as Scope[]).map((s) => (
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
                {s === '' ? 'All scopes' : s.charAt(0).toUpperCase() + s.slice(1)}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Graph + side panel */}
      <div className="flex flex-1 min-h-0">
        <div className="flex-1 min-w-0 relative">
          {isLoading ? (
            <div className="p-6">
              <Skeleton className="h-[600px] rounded-xl" />
            </div>
          ) : !blocks?.length ? (
            <div className="flex items-center justify-center h-full">
              <div className="text-center">
                <p className="text-sm text-sera-text-muted mb-1">No memory blocks yet.</p>
                <p className="text-xs text-sera-text-dim">
                  Memory blocks will appear here once the agent has learned something.
                </p>
              </div>
            </div>
          ) : (
            <div className="p-4 h-full">
              <Suspense fallback={<Skeleton className="h-[600px] rounded-xl" />}>
                <MemoryGraph
                  data={graphData}
                  onNodeClick={handleNodeClick}
                  onNodeDoubleClick={handleNodeDoubleClick}
                  className="h-full"
                />
              </Suspense>
            </div>
          )}
        </div>

        {/* Side panel */}
        {selectedBlock && (
          <div className="w-72 border-l border-sera-border flex flex-col flex-shrink-0 overflow-y-auto">
            <div className="flex items-center justify-between px-4 py-3 border-b border-sera-border">
              <span className="text-sm font-semibold text-sera-text truncate">
                {selectedBlock.title}
              </span>
              <button
                onClick={() => setSelectedBlock(null)}
                className="p-1 text-sera-text-muted hover:text-sera-text transition-colors flex-shrink-0"
              >
                <X size={13} />
              </button>
            </div>

            <div className="p-4 space-y-4">
              <div className="flex flex-wrap gap-1.5">
                <Badge variant="accent">{selectedBlock.type}</Badge>
                <Badge variant="default">{selectedBlock.scope}</Badge>
              </div>

              {selectedBlock.tags && selectedBlock.tags.length > 0 && (
                <div>
                  <div className="flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                    <Tag size={9} /> Tags
                  </div>
                  <div className="flex flex-wrap gap-1">
                    {selectedBlock.tags.map((tag) => (
                      <span
                        key={tag}
                        className="text-[10px] text-sera-text-dim bg-sera-surface-active px-1.5 py-0.5 rounded"
                      >
                        {tag}
                      </span>
                    ))}
                  </div>
                </div>
              )}

              {selectedBlock.content && (
                <div>
                  <div className="text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                    Content
                  </div>
                  <p className="text-xs text-sera-text-muted leading-relaxed whitespace-pre-wrap">
                    {selectedBlock.content}
                  </p>
                </div>
              )}

              {selectedBlock.updatedAt && (
                <div className="flex items-center gap-1 text-[10px] text-sera-text-dim">
                  <Clock size={9} /> Updated {new Date(selectedBlock.updatedAt).toLocaleString()}
                </div>
              )}

              <Link
                to={`/memory/${selectedBlock.id}`}
                className="block w-full text-center text-xs text-sera-accent hover:underline mt-2"
              >
                Open full view →
              </Link>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
