import { useEffect, useRef, useState, useMemo, useCallback } from 'react';
import ForceGraph2D, { ForceGraphMethods } from 'react-force-graph-2d';
import { useNavigate } from 'react-router';

export interface GraphNode {
  id: string;
  title: string;
  type: string;
  tags: string[];
}

export interface GraphEdge {
  from: string;
  to: string;
  kind: 'ref' | 'wikilink';
}

export interface MemoryGraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface MemoryGraphProps {
  data: MemoryGraphData;
  onNodeClick?: (node: GraphNode) => void;
  onNodeDoubleClick?: (node: GraphNode) => void;
  searchQuery?: string;
  className?: string;
}

const TYPE_COLORS: Record<string, string> = {
  // Epic 8 block types
  fact: '#3b82f6', // blue
  context: '#a855f7', // purple
  memory: '#22c55e', // green
  insight: '#eab308', // yellow
  reference: '#06b6d4', // cyan
  observation: '#f97316', // orange
  decision: '#ef4444', // red
  // Legacy types
  human: '#60a5fa', // light blue
  persona: '#c084fc', // light purple
  core: '#4ade80', // light green
  archive: '#6b7280', // gray
  // Meta-node types (agent/circle)
  agent: '#f472b6', // pink
  circle: '#a78bfa', // violet
};

const DEFAULT_COLOR = '#9ca3af';

/** Node radius by kind — agents and circles are larger. */
function nodeRadius(type: string): number {
  if (type === 'circle') return 10;
  if (type === 'agent') return 8;
  return 5;
}

export default function MemoryGraph({
  data,
  onNodeClick,
  onNodeDoubleClick,
  searchQuery = '',
  className = '',
}: MemoryGraphProps): React.JSX.Element {
  const fgRef = useRef<ForceGraphMethods | undefined>(undefined);
  const navigate = useNavigate();
  const [dimensions, setDimensions] = useState({ width: 800, height: 600 });
  const containerRef = useRef<HTMLDivElement>(null);

  // Update dimensions on resize
  useEffect(() => {
    const updateDimensions = () => {
      if (containerRef.current) {
        const { clientWidth, clientHeight } = containerRef.current;
        setDimensions({
          width: clientWidth,
          height: clientHeight || 600,
        });
      }
    };

    updateDimensions();
    window.addEventListener('resize', updateDimensions);
    return () => window.removeEventListener('resize', updateDimensions);
  }, []);

  const lastClickRef = useRef<{ id: string; time: number } | null>(null);

  const handleNodeDoubleClick = useCallback(
    (node: GraphNode) => {
      if (onNodeDoubleClick) {
        onNodeDoubleClick(node);
      } else {
        void navigate(`/memory/${node.id}`);
      }
    },
    [onNodeDoubleClick, navigate]
  );

  const handleNodeClick = useCallback(
    (node: GraphNode) => {
      const now = Date.now();
      if (
        lastClickRef.current &&
        lastClickRef.current.id === node.id &&
        now - lastClickRef.current.time < 300
      ) {
        // Double click
        handleNodeDoubleClick(node);
        lastClickRef.current = null;
      } else {
        // Single click
        lastClickRef.current = { id: node.id, time: now };
        if (onNodeClick) {
          onNodeClick(node);
        }
      }
    },
    [onNodeClick, handleNodeDoubleClick]
  );

  // Transform data to fit react-force-graph
  const graphData = useMemo(() => {
    return {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: data.edges.map((e) => ({ source: e.from, target: e.to, kind: e.kind })),
    };
  }, [data]);

  // Handle node rendering and coloring based on search and type
  const nodeCanvasObject = useCallback(
    (nodeObj: object, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const node = nodeObj as GraphNode & { x?: number; y?: number };
      const label = node.title || '';
      const fontSize = 12 / globalScale;
      ctx.font = `${fontSize}px Sans-Serif`;

      const isMatched = searchQuery
        ? label.toLowerCase().includes(searchQuery.toLowerCase()) ||
          (node.tags &&
            node.tags.some((t: string) => t.toLowerCase().includes(searchQuery.toLowerCase())))
        : true;

      const color = TYPE_COLORS[node.type] || DEFAULT_COLOR;
      const r = nodeRadius(node.type);
      const x = node.x || 0;
      const y = node.y || 0;

      // Draw node
      ctx.beginPath();
      if (node.type === 'agent') {
        // Hexagon for agents
        for (let i = 0; i < 6; i++) {
          const angle = (Math.PI / 3) * i - Math.PI / 6;
          const px = x + r * Math.cos(angle);
          const py = y + r * Math.sin(angle);
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.closePath();
      } else if (node.type === 'circle') {
        // Double circle for circles
        ctx.arc(x, y, r, 0, 2 * Math.PI, false);
        ctx.moveTo(x + r * 0.7, y);
        ctx.arc(x, y, r * 0.7, 0, 2 * Math.PI, false);
      } else {
        ctx.arc(x, y, r, 0, 2 * Math.PI, false);
      }
      ctx.fillStyle = isMatched ? color : '#374151';
      ctx.fill();

      // Node border
      if (searchQuery && isMatched) {
        ctx.lineWidth = 1.5 / globalScale;
        ctx.strokeStyle = '#ffffff';
        ctx.stroke();
      } else if (node.type === 'agent' || node.type === 'circle') {
        ctx.lineWidth = 1 / globalScale;
        ctx.strokeStyle = color;
        ctx.stroke();
      }

      // Draw text label
      const textWidth = ctx.measureText(label).width;
      const bckgDimensions = [textWidth, fontSize].map((n) => n + fontSize * 0.2);

      // Label background
      if (isMatched) {
        ctx.fillStyle = 'rgba(15, 23, 42, 0.8)';
        ctx.fillRect(
          (node.x || 0) - bckgDimensions[0] / 2,
          (node.y || 0) + 6,
          bckgDimensions[0],
          bckgDimensions[1]
        );

        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillStyle = '#e2e8f0';
        ctx.fillText(label, node.x || 0, (node.y || 0) + 6 + fontSize / 2);
      }
    },
    [searchQuery]
  );

  return (
    <div
      ref={containerRef}
      className={`w-full h-[600px] border border-sera-border rounded-lg overflow-hidden bg-[#0a0a0a] relative ${className}`}
    >
      <ForceGraph2D
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ref={fgRef as any}
        width={dimensions.width}
        height={dimensions.height}
        graphData={graphData}
        nodeLabel={(nodeObj: object) => {
          const node = nodeObj as GraphNode;
          const lines = [node.title];
          if (node.type) lines.push(`Type: ${node.type}`);
          if (node.tags?.length) lines.push(`Tags: ${node.tags.slice(0, 5).join(', ')}`);
          return lines.join('\n');
        }}
        nodeRelSize={5}
        nodeCanvasObject={nodeCanvasObject}
        linkColor={(link: object) =>
          (link as GraphEdge).kind === 'wikilink'
            ? 'rgba(100, 116, 139, 0.5)'
            : 'rgba(148, 163, 184, 0.6)'
        }
        linkWidth={(link: object) => ((link as GraphEdge).kind === 'wikilink' ? 1 : 1.5)}
        linkLineDash={(link: object) => ((link as GraphEdge).kind === 'wikilink' ? [2, 2] : null)}
        linkLabel={(link: object) => (link as GraphEdge).kind}
        linkDirectionalArrowLength={4}
        linkDirectionalArrowRelPos={0.85}
        linkDirectionalParticles={2}
        linkDirectionalParticleWidth={2}
        linkDirectionalParticleSpeed={0.005}
        linkDirectionalParticleColor={(link: object) =>
          (link as GraphEdge).kind === 'wikilink'
            ? 'rgba(168, 85, 247, 0.7)'
            : 'rgba(148, 163, 184, 0.8)'
        }
        onNodeClick={(node: object) => handleNodeClick(node as GraphNode)}
        d3AlphaDecay={0.02}
        d3VelocityDecay={0.3}
      />

      {/* Legend */}
      <div className="absolute top-4 left-4 bg-sera-surface/80 backdrop-blur-sm border border-sera-border p-3 rounded-md text-xs max-h-64 overflow-y-auto">
        <h4 className="font-semibold text-sera-text mb-2">Node Types</h4>
        <div className="flex flex-col gap-1">
          {(
            [
              'agent',
              'circle',
              'fact',
              'context',
              'memory',
              'insight',
              'reference',
              'observation',
              'decision',
            ] as const
          ).map((type) => (
            <div key={type} className="flex items-center gap-2">
              <span
                className={`w-3 h-3 shrink-0 ${type === 'agent' ? 'rotate-45' : 'rounded-full'}`}
                style={{ backgroundColor: TYPE_COLORS[type] }}
              />
              <span className="text-sera-text-muted capitalize">{type}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
