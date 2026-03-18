'use client';

import { useState, useEffect, useMemo } from 'react';
import Link from 'next/link';
import {
  Search,
  Filter,
  Database,
  User,
  Brain,
  Archive,
  Tag as TagIcon,
  RefreshCw,
  AlertCircle,
  ChevronRight,
  Clock
} from 'lucide-react';

interface MemoryEntry {
  id: string;
  title: string;
  type: 'human' | 'persona' | 'core' | 'archive';
  content: string;
  refs: string[];
  tags: string[];
  source: string;
  createdAt: string;
  updatedAt: string;
}

interface MemoryBlock {
  type: string;
  entries: MemoryEntry[];
}

const TYPE_ICONS: Record<string, React.ReactNode> = {
  human: <User size={16} />,
  persona: <Brain size={16} />,
  core: <Database size={16} />,
  archive: <Archive size={16} />,
};

const TYPE_COLORS: Record<string, string> = {
  human: 'text-blue-500 bg-blue-500/10 border-blue-500/20',
  persona: 'text-purple-500 bg-purple-500/10 border-purple-500/20',
  core: 'text-amber-500 bg-amber-500/10 border-amber-500/20',
  archive: 'text-sera-text-muted bg-sera-surface border-sera-border',
};

export default function MemoryPage() {
  const [blocks, setBlocks] = useState<MemoryBlock[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedType, setSelectedType] = useState<string | 'all'>('all');

  const fetchMemory = async () => {
    try {
      setLoading(true);
      const res = await fetch('/api/core/memory/blocks');
      if (!res.ok) throw new Error('Failed to fetch memory blocks');
      const data = await res.json();
      setBlocks(data);
      setError(null);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchMemory();
  }, []);

  const allEntries = useMemo(() => {
    return blocks.flatMap(block => block.entries);
  }, [blocks]);

  const filteredEntries = useMemo(() => {
    return allEntries.filter(entry => {
      const matchesSearch =
        entry.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
        entry.content.toLowerCase().includes(searchQuery.toLowerCase()) ||
        entry.tags.some(tag => tag.toLowerCase().includes(searchQuery.toLowerCase()));

      const matchesType = selectedType === 'all' || entry.type === selectedType;

      return matchesSearch && matchesType;
    }).sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime());
  }, [allEntries, searchQuery, selectedType]);

  const allTags = useMemo(() => {
    const tags = new Set<string>();
    allEntries.forEach(entry => entry.tags.forEach(tag => tags.add(tag)));
    return Array.from(tags).sort();
  }, [allEntries]);

  return (
    <div className="p-8 max-w-6xl mx-auto h-full flex flex-col">
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 mb-8">
        <div>
          <h1 className="text-2xl font-bold text-sera-text">Memory Browse</h1>
          <p className="text-sm text-sera-text-muted mt-1">Explore and manage agent memory entries</p>
        </div>

        <div className="flex items-center gap-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-sera-text-dim" size={18} />
            <input
              type="text"
              placeholder="Search memory..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-10 pr-4 py-2 bg-sera-surface border border-sera-border rounded-lg text-sm text-sera-text focus:outline-none focus:border-sera-accent w-64"
            />
          </div>
          <button
            onClick={fetchMemory}
            className="p-2 hover:bg-sera-surface rounded-lg transition-colors text-sera-text-muted hover:text-sera-text"
          >
            <RefreshCw size={20} className={loading ? 'animate-spin' : ''} />
          </button>
        </div>
      </div>

      <div className="flex gap-4 mb-6 overflow-x-auto pb-2">
        <button
          onClick={() => setSelectedType('all')}
          className={`px-4 py-1.5 rounded-full text-xs font-medium transition-all border ${
            selectedType === 'all'
              ? 'bg-sera-accent text-white border-sera-accent'
              : 'bg-sera-surface text-sera-text-muted border-sera-border hover:border-sera-text-dim'
          }`}
        >
          All Entries
        </button>
        {['human', 'persona', 'core', 'archive'].map(type => (
          <button
            key={type}
            onClick={() => setSelectedType(type)}
            className={`px-4 py-1.5 rounded-full text-xs font-medium transition-all border flex items-center gap-2 ${
              selectedType === type
                ? 'bg-sera-accent text-white border-sera-accent'
                : 'bg-sera-surface text-sera-text-muted border-sera-border hover:border-sera-text-dim'
            }`}
          >
            {TYPE_ICONS[type]}
            <span className="capitalize">{type}</span>
          </button>
        ))}
      </div>

      {loading && blocks.length === 0 ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="animate-pulse flex flex-col items-center gap-4">
            <Database size={40} className="text-sera-border" />
            <p className="text-sera-text-muted">Loading memory entries...</p>
          </div>
        </div>
      ) : error ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="p-6 border border-sera-error/30 bg-sera-error/5 rounded-lg flex flex-col items-center gap-3 text-sera-error max-w-md text-center">
            <AlertCircle size={32} />
            <p className="font-medium">Failed to load memory</p>
            <p className="text-sm opacity-80">{error}</p>
            <button onClick={fetchMemory} className="mt-2 text-sm underline hover:no-underline">Try again</button>
          </div>
        </div>
      ) : filteredEntries.length === 0 ? (
        <div className="flex-1 flex flex-col items-center justify-center py-20 bg-sera-surface/30 border border-dashed border-sera-border rounded-xl">
          <Search size={40} className="text-sera-text-dim mb-4" />
          <h3 className="text-lg font-semibold text-sera-text">No entries found</h3>
          <p className="text-sm text-sera-text-muted max-w-xs text-center mt-2">
            Try adjusting your search or filters to find what you're looking for.
          </p>
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto pr-2 -mr-2 space-y-3">
          {filteredEntries.map(entry => (
            <Link
              key={entry.id}
              href={`/memory/${entry.id}`}
              className="block group"
            >
              <div className="p-5 bg-sera-surface border border-sera-border rounded-xl hover:border-sera-accent/50 transition-all hover:shadow-sm">
                <div className="flex items-start justify-between gap-4">
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-3 mb-2">
                      <span className={`px-2 py-0.5 rounded text-[10px] font-bold uppercase tracking-wider border flex items-center gap-1.5 ${TYPE_COLORS[entry.type]}`}>
                        {TYPE_ICONS[entry.type]}
                        {entry.type}
                      </span>
                      <span className="text-[10px] text-sera-text-dim flex items-center gap-1 font-mono">
                        <Clock size={10} />
                        {new Date(entry.updatedAt).toLocaleDateString()}
                      </span>
                    </div>
                    <h3 className="text-base font-semibold text-sera-text group-hover:text-sera-accent transition-colors truncate">
                      {entry.title}
                    </h3>
                    <p className="text-sm text-sera-text-muted mt-1.5 line-clamp-2 leading-relaxed">
                      {entry.content}
                    </p>

                    {entry.tags.length > 0 && (
                      <div className="flex flex-wrap gap-2 mt-4">
                        {entry.tags.map(tag => (
                          <span key={tag} className="text-[10px] px-2 py-0.5 bg-sera-bg border border-sera-border rounded text-sera-text-dim flex items-center gap-1">
                            <TagIcon size={8} />
                            {tag}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>

                  <div className="text-sera-text-dim group-hover:text-sera-accent transition-colors mt-1 self-center">
                    <ChevronRight size={20} />
                  </div>
                </div>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
