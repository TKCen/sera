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
  human: '#3b82f6', // blue
  persona: '#a855f7', // purple
  core: '#22c55e', // green
  archive: '#6b7280', // gray
};

const DEFAULT_COLOR = '#9ca3af';

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
            node.tags.some((t: string) =>
              t.toLowerCase().includes(searchQuery.toLowerCase())
            ))
        : true;

      const color = TYPE_COLORS[node.type] || DEFAULT_COLOR;

      // Draw node circle
      ctx.beginPath();
      ctx.arc(node.x || 0, node.y || 0, 5, 0, 2 * Math.PI, false);
      ctx.fillStyle = isMatched ? color : '#374151'; // highlight or dim
      ctx.fill();

      // Node border for matches if searching
      if (searchQuery && isMatched) {
        ctx.lineWidth = 1.5 / globalScale;
        ctx.strokeStyle = '#ffffff';
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
        nodeLabel="title"
        nodeRelSize={5}
        nodeCanvasObject={nodeCanvasObject}
        linkColor={(link: object) =>
          (link as GraphEdge).kind === 'wikilink' ? 'rgba(100, 116, 139, 0.5)' : 'rgba(148, 163, 184, 0.6)'
        }
        linkWidth={(link: object) => ((link as GraphEdge).kind === 'wikilink' ? 1 : 1.5)}
        linkLineDash={(link: object) => ((link as GraphEdge).kind === 'wikilink' ? [2, 2] : null)}
        onNodeClick={(node: object) => handleNodeClick(node as GraphNode)}
        d3AlphaDecay={0.02}
        d3VelocityDecay={0.3}
      />

      {/* Legend */}
      <div className="absolute top-4 left-4 bg-sera-surface/80 backdrop-blur-sm border border-sera-border p-3 rounded-md text-xs">
        <h4 className="font-semibold text-sera-text mb-2">Node Types</h4>
        <div className="flex flex-col gap-1.5">
          {Object.entries(TYPE_COLORS).map(([type, color]) => (
            <div key={type} className="flex items-center gap-2">
              <span className="w-3 h-3 rounded-full" style={{ backgroundColor: color }}></span>
              <span className="text-sera-text-muted capitalize">{type}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
