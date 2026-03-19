"use client";

import { useEffect, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { ArrowLeft } from "lucide-react";

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

export default function MemoryEntryPage() {
  const params = useParams();
  const router = useRouter();
  const id = params.id as string;

  const [entry, setEntry] = useState<MemoryEntry | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    async function fetchEntry() {
      if (!id) return;
      try {
        setLoading(true);
        const res = await fetch(`/api/core/memory/entries/${id}`);
        if (!res.ok) {
          throw new Error("Failed to load memory entry");
        }
        const data = await res.json();
        setEntry(data);
      } catch (err: unknown) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLoading(false);
      }
    }

    fetchEntry();
  }, [id]);

  return (
    <div className="p-8 max-w-5xl mx-auto h-full flex flex-col">
      <div className="flex items-center space-x-4 mb-6">
        <button
          onClick={() => router.back()}
          className="p-2 hover:bg-sera-surface rounded-full transition-colors text-sera-text-muted hover:text-sera-text"
        >
          <ArrowLeft size={20} />
        </button>
        <div>
          <h1 className="text-2xl font-bold text-sera-text">
            {loading ? "Loading..." : entry?.title || "Entry Not Found"}
          </h1>
          {entry && (
            <div className="flex space-x-2 mt-2">
              <span className="text-xs px-2 py-0.5 rounded-full bg-sera-surface text-sera-text-muted border border-sera-border">
                {entry.type}
              </span>
              <span className="text-xs px-2 py-0.5 rounded-full bg-sera-surface text-sera-text-muted border border-sera-border">
                {entry.source}
              </span>
            </div>
          )}
        </div>
      </div>

      {loading && (
        <div className="flex-1 flex items-center justify-center">
          <div className="animate-pulse text-sera-text-muted">Loading entry...</div>
        </div>
      )}

      {error && (
        <div className="flex-1 flex items-center justify-center text-red-500">
          {error}
        </div>
      )}

      {entry && !loading && !error && (
        <div className="flex-1 flex flex-col gap-6">
          <div className="p-6 bg-sera-surface border border-sera-border rounded-lg shadow-sm font-mono text-sm text-sera-text whitespace-pre-wrap overflow-y-auto max-h-[60vh]">
            {entry.content}
          </div>

          <div className="grid grid-cols-2 gap-6">
            <div className="p-6 bg-sera-surface border border-sera-border rounded-lg">
              <h3 className="text-sm font-semibold text-sera-text mb-3">Metadata</h3>
              <div className="space-y-2 text-sm text-sera-text-muted">
                <p><strong>ID:</strong> {entry.id}</p>
                <p><strong>Created:</strong> {new Date(entry.createdAt).toLocaleString()}</p>
                <p><strong>Updated:</strong> {new Date(entry.updatedAt).toLocaleString()}</p>
              </div>
            </div>

            <div className="p-6 bg-sera-surface border border-sera-border rounded-lg">
              <h3 className="text-sm font-semibold text-sera-text mb-3">Tags & Refs</h3>
              <div className="mb-4">
                <h4 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-2">Tags</h4>
                {entry.tags.length > 0 ? (
                  <div className="flex flex-wrap gap-2">
                    {entry.tags.map(tag => (
                      <span key={tag} className="text-xs px-2 py-1 bg-sera-bg border border-sera-border rounded text-sera-text-muted">
                        #{tag}
                      </span>
                    ))}
                  </div>
                ) : (
                  <p className="text-xs text-sera-text-dim">No tags.</p>
                )}
              </div>

              <div>
                <h4 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-2">Refs</h4>
                {entry.refs.length > 0 ? (
                  <ul className="list-disc list-inside text-sm text-sera-text-muted">
                    {entry.refs.map(ref => (
                      <li key={ref}>
                        <Link href={`/memory/${ref}`} className="hover:text-sera-text hover:underline transition-colors">{ref}</Link>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="text-xs text-sera-text-dim">No references.</p>
                )}
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
