"use client";

import { useEffect, useState, useMemo } from 'react';
import Link from 'next/link';
import { Search, Filter, X, Tag } from 'lucide-react';
import { GraphNode, MemoryGraphData } from "@/components/MemoryGraph";
import MemoryGraphWrapper from "@/components/MemoryGraphWrapper";

interface MemoryEntry {
  id: string;
  title: string;
  type: string;
  content: string;
  refs: string[];
  tags: string[];
  source: string;
  createdAt: string;
  updatedAt: string;
}

export default function InsightsPage() {
  const [graphData, setGraphData] = useState<MemoryGraphData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Filtering and Search State
  const [searchQuery, setSearchQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<string>("all");
  const [tagFilter, setTagFilter] = useState<string>("all");

  // Selected Node State
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [selectedEntry, setSelectedEntry] = useState<MemoryEntry | null>(null);
  const [entryLoading, setEntryLoading] = useState(false);

  useEffect(() => {
    async function fetchGraph() {
      try {
        setLoading(true);
        const res = await fetch('/api/core/memory/graph');
        if (!res.ok) {
          throw new Error('Failed to load memory graph');
        }
        const data = await res.json();
        setGraphData(data);
      } catch (err: unknown) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLoading(false);
      }
    }
    fetchGraph();
  }, []);

  useEffect(() => {
    async function fetchEntry() {
      if (!selectedNode) {
        setSelectedEntry(null);
        return;
      }
      try {
        setEntryLoading(true);
        const res = await fetch(`/api/core/memory/entries/${selectedNode.id}`);
        if (!res.ok) {
          throw new Error('Failed to load entry');
        }
        const data = await res.json();
        setSelectedEntry(data);
      } catch (err: unknown) {
        console.error(err);
        setSelectedEntry(null);
      } finally {
        setEntryLoading(false);
      }
    }
    fetchEntry();
  }, [selectedNode]);

  // Extract all unique tags
  const allTags = useMemo(() => {
    if (!graphData) return [];
    const tags = new Set<string>();
    graphData.nodes.forEach(n => {
      n.tags?.forEach(t => tags.add(t));
    });
    return Array.from(tags).sort();
  }, [graphData]);

  const filteredData = useMemo(() => {
    if (!graphData) return { nodes: [], edges: [] };

    let nodes = graphData.nodes;

    if (typeFilter !== 'all') {
      nodes = nodes.filter(n => n.type === typeFilter);
    }

    if (tagFilter !== 'all') {
      nodes = nodes.filter(n => n.tags && n.tags.includes(tagFilter));
    }

    if (searchQuery) {
      const query = searchQuery.toLowerCase();
      nodes = nodes.filter(n =>
        (n.title && n.title.toLowerCase().includes(query)) ||
        (n.tags && n.tags.some(t => t.toLowerCase().includes(query)))
      );
    }

    // Edges are only kept if both from and to are in the filtered nodes
    const nodeIds = new Set(nodes.map(n => n.id));
    const edges = graphData.edges.filter(e => nodeIds.has(e.from) && nodeIds.has(e.to));

    return { nodes, edges };
  }, [graphData, typeFilter, tagFilter, searchQuery]);

  const handleNodeClick = (node: GraphNode) => {
    setSelectedNode(node);
  };

  const closePanel = () => {
    setSelectedNode(null);
  };

  return (
    <div className="p-8 max-w-[1400px] mx-auto h-full flex flex-col">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Knowledge Graph</h1>
          <p className="text-sm text-sera-text-muted mt-1">Interactive visualization of your agents&apos; memories and their connections.</p>
        </div>
      </div>

      <div className="flex-1 flex gap-6 mt-4 min-h-0">
        {/* Main Graph Area */}
        <div className="flex-1 flex flex-col bg-sera-surface border border-sera-border rounded-xl shadow-sm overflow-hidden relative">

          {/* Controls Bar */}
          <div className="p-4 border-b border-sera-border flex gap-4 bg-sera-bg/50 items-center justify-between z-10 flex-wrap">
            <div className="relative flex-1 min-w-[200px] max-w-md">
              <Search size={18} className="absolute left-3 top-1/2 -translate-y-1/2 text-sera-text-muted" />
              <input
                type="text"
                placeholder="Search memories by title or tags..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full pl-10 pr-4 py-2 bg-sera-bg border border-sera-border rounded-lg text-sm text-sera-text placeholder:text-sera-text-dim focus:outline-none focus:border-sera-primary"
              />
              {searchQuery && (
                <button
                  onClick={() => setSearchQuery("")}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-sera-text-muted hover:text-sera-text"
                >
                  <X size={14} />
                </button>
              )}
            </div>

            <div className="flex items-center gap-4 flex-wrap">
              <div className="flex items-center gap-2">
                <Tag size={18} className="text-sera-text-muted" />
                <select
                  value={tagFilter}
                  onChange={(e) => setTagFilter(e.target.value)}
                  className="bg-sera-bg border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-primary appearance-none cursor-pointer pr-8 max-w-[150px]"
                >
                  <option value="all">All Tags</option>
                  {allTags.map(t => (
                    <option key={t} value={t}>{t}</option>
                  ))}
                </select>
              </div>

              <div className="flex items-center gap-2">
                <Filter size={18} className="text-sera-text-muted" />
                <select
                  value={typeFilter}
                  onChange={(e) => setTypeFilter(e.target.value)}
                  className="bg-sera-bg border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-primary appearance-none cursor-pointer pr-8"
                >
                  <option value="all">All Types</option>
                  <option value="human">Human</option>
                  <option value="persona">Persona</option>
                  <option value="core">Core</option>
                  <option value="archive">Archive</option>
                </select>
              </div>
            </div>
          </div>

          {/* Graph Container */}
          <div className="flex-1 relative">
            {loading ? (
              <div className="absolute inset-0 flex items-center justify-center">
                <div className="animate-pulse text-sera-text-muted flex flex-col items-center gap-4">
                  <div className="w-8 h-8 rounded-full border-2 border-sera-primary border-t-transparent animate-spin"></div>
                  Loading graph data...
                </div>
              </div>
            ) : error ? (
              <div className="absolute inset-0 flex items-center justify-center text-red-500 p-6 text-center">
                Error loading graph: {error}
              </div>
            ) : graphData ? (
              <MemoryGraphWrapper
                data={filteredData}
                searchQuery={searchQuery}
                onNodeClick={handleNodeClick}
                className="rounded-none border-none border-t border-sera-border h-full absolute inset-0"
              />
            ) : null}
          </div>
        </div>

        {/* Side Panel for Node Details */}
        {selectedNode && (
          <div className="w-80 bg-sera-surface border border-sera-border rounded-xl shadow-sm flex flex-col overflow-hidden animate-in slide-in-from-right-4 duration-200">
            <div className="p-4 border-b border-sera-border flex justify-between items-start bg-sera-bg/50">
              <div>
                <h3 className="font-medium text-sera-text truncate max-w-[220px]" title={selectedNode.title}>
                  {selectedNode.title}
                </h3>
                <span className="text-xs px-2 py-0.5 rounded-full bg-sera-bg border border-sera-border text-sera-text-muted mt-2 inline-block capitalize">
                  {selectedNode.type}
                </span>
              </div>
              <button onClick={closePanel} className="text-sera-text-muted hover:text-sera-text p-1">
                <X size={16} />
              </button>
            </div>

            <div className="p-4 flex-1 overflow-y-auto">
              <div className="mb-4">
                <h4 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-2">Tags</h4>
                {selectedNode.tags && selectedNode.tags.length > 0 ? (
                  <div className="flex flex-wrap gap-1.5">
                    {selectedNode.tags.map(tag => (
                      <span key={tag} className="text-[10px] px-1.5 py-0.5 bg-sera-bg border border-sera-border rounded text-sera-text-muted">
                        #{tag}
                      </span>
                    ))}
                  </div>
                ) : (
                  <p className="text-xs text-sera-text-dim italic">No tags</p>
                )}
              </div>

              <div className="mb-4">
                <h4 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-2">Content Preview</h4>
                {entryLoading ? (
                  <div className="animate-pulse flex flex-col gap-2">
                    <div className="h-3 bg-sera-bg rounded w-3/4"></div>
                    <div className="h-3 bg-sera-bg rounded w-full"></div>
                    <div className="h-3 bg-sera-bg rounded w-5/6"></div>
                  </div>
                ) : selectedEntry ? (
                  <div className="text-sm text-sera-text-muted bg-sera-bg p-3 rounded-lg border border-sera-border max-h-48 overflow-y-auto whitespace-pre-wrap font-mono text-xs">
                    {selectedEntry.content}
                  </div>
                ) : (
                  <p className="text-xs text-sera-text-dim italic">Failed to load entry content.</p>
                )}
              </div>

              <div className="mt-auto pt-4 flex flex-col items-center">
                 <p className="text-xs text-sera-text-dim text-center mb-2">
                   Double-click node to open full details
                 </p>
                 <Link
                   href={`/memory/${selectedNode.id}`}
                   className="w-full py-2 bg-sera-bg hover:bg-sera-surface border border-sera-border rounded-lg text-sm text-sera-text text-center transition-colors block"
                 >
                   Open Entry
                 </Link>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
