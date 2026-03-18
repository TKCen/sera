"use client";

import { useEffect, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { ArrowLeft, Edit2, Trash2, Save, X, Clock, Database, User, Brain, Archive } from "lucide-react";

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

const TYPE_ICONS: Record<string, React.ReactNode> = {
  human: <User size={16} />,
  persona: <Brain size={16} />,
  core: <Database size={16} />,
  archive: <Archive size={16} />,
};

export default function MemoryEntryPage() {
  const params = useParams();
  const router = useRouter();
  const id = params.id as string;

  const [entry, setEntry] = useState<MemoryEntry | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isEditing, setIsEditing] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [saving, setSaving] = useState(false);

  const fetchEntry = async () => {
    if (!id) return;
    try {
      setLoading(true);
      const res = await fetch(`/api/core/memory/entries/${id}`);
      if (!res.ok) {
        throw new Error("Failed to load memory entry");
      }
      const data = await res.json();
      setEntry(data);
      setEditContent(data.content);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchEntry();
  }, [id]);

  const handleSave = async () => {
    try {
      setSaving(true);
      const res = await fetch(`/api/core/memory/entries/${id}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content: editContent }),
      });
      if (!res.ok) throw new Error("Failed to save changes");
      const updated = await res.json();
      setEntry(updated);
      setIsEditing(false);
    } catch (err: any) {
      alert(err.message);
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!confirm("Are you sure you want to delete this memory entry?")) return;
    try {
      const res = await fetch(`/api/core/memory/entries/${id}`, {
        method: "DELETE",
      });
      if (!res.ok) throw new Error("Failed to delete entry");
      router.push("/memory");
    } catch (err: any) {
      alert(err.message);
    }
  };

  return (
    <div className="p-8 max-w-5xl mx-auto h-full flex flex-col">
      <div className="flex items-center justify-between mb-8">
        <div className="flex items-center space-x-4">
          <button
            onClick={() => router.push("/memory")}
            className="p-2 hover:bg-sera-surface rounded-full transition-colors text-sera-text-muted hover:text-sera-text"
          >
            <ArrowLeft size={20} />
          </button>
          <div>
            <h1 className="text-2xl font-bold text-sera-text">
              {loading ? "Loading..." : entry?.title || "Entry Not Found"}
            </h1>
            {entry && (
              <div className="flex items-center space-x-3 mt-2">
                <span className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider px-2 py-0.5 rounded bg-sera-surface text-sera-text-muted border border-sera-border">
                  {TYPE_ICONS[entry.type]}
                  {entry.type}
                </span>
                <span className="text-[10px] text-sera-text-dim flex items-center gap-1 font-mono">
                  <Clock size={10} />
                  {new Date(entry.updatedAt).toLocaleString()}
                </span>
              </div>
            )}
          </div>
        </div>

        {entry && !loading && (
          <div className="flex items-center gap-2">
            {isEditing ? (
              <>
                <button
                  onClick={() => {
                    setIsEditing(false);
                    setEditContent(entry.content);
                  }}
                  className="sera-btn-ghost flex items-center gap-2 px-3 py-1.5 text-xs"
                  disabled={saving}
                >
                  <X size={14} />
                  Cancel
                </button>
                <button
                  onClick={handleSave}
                  className="sera-btn-primary flex items-center gap-2 px-3 py-1.5 text-xs"
                  disabled={saving}
                >
                  <Save size={14} />
                  {saving ? "Saving..." : "Save Changes"}
                </button>
              </>
            ) : (
              <>
                <button
                  onClick={() => setIsEditing(true)}
                  className="p-2 text-sera-text-dim hover:text-sera-accent hover:bg-sera-accent/10 rounded-md transition-colors"
                  title="Edit"
                >
                  <Edit2 size={18} />
                </button>
                <button
                  onClick={handleDelete}
                  className="p-2 text-sera-text-dim hover:text-sera-error hover:bg-sera-error/10 rounded-md transition-colors"
                  title="Delete"
                >
                  <Trash2 size={18} />
                </button>
              </>
            )}
          </div>
        )}
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
          <div className="flex-1 min-h-0 flex flex-col">
            {isEditing ? (
              <textarea
                value={editContent}
                onChange={(e) => setEditContent(e.target.value)}
                className="flex-1 p-6 bg-sera-bg border border-sera-accent/30 rounded-lg font-mono text-sm text-sera-text focus:outline-none focus:border-sera-accent transition-colors resize-none"
                placeholder="Memory content..."
              />
            ) : (
              <div className="p-6 bg-sera-surface border border-sera-border rounded-lg shadow-sm font-mono text-sm text-sera-text whitespace-pre-wrap overflow-y-auto">
                {entry.content}
              </div>
            )}
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            <div className="p-5 bg-sera-surface border border-sera-border rounded-xl">
              <h3 className="text-xs font-semibold uppercase tracking-widest text-sera-text-dim mb-4">Metadata</h3>
              <div className="space-y-3 text-sm text-sera-text-muted">
                <div className="flex justify-between">
                  <span className="text-sera-text-dim">ID</span>
                  <span className="font-mono text-[11px]">{entry.id}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-sera-text-dim">Source</span>
                  <span className="capitalize">{entry.source}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-sera-text-dim">Created</span>
                  <span>{new Date(entry.createdAt).toLocaleString()}</span>
                </div>
              </div>
            </div>

            <div className="p-5 bg-sera-surface border border-sera-border rounded-xl">
              <h3 className="text-xs font-semibold uppercase tracking-widest text-sera-text-dim mb-4">Tags & Refs</h3>
              <div className="space-y-4">
                <div>
                  <h4 className="text-[10px] font-bold text-sera-text-dim uppercase tracking-tighter mb-2">Tags</h4>
                  {entry.tags.length > 0 ? (
                    <div className="flex flex-wrap gap-2">
                      {entry.tags.map(tag => (
                        <span key={tag} className="text-[10px] px-2 py-1 bg-sera-bg border border-sera-border rounded text-sera-text-muted">
                          #{tag}
                        </span>
                      ))}
                    </div>
                  ) : (
                    <p className="text-xs text-sera-text-dim italic">No tags assigned.</p>
                  )}
                </div>

                <div>
                  <h4 className="text-[10px] font-bold text-sera-text-dim uppercase tracking-tighter mb-2">References</h4>
                  {entry.refs.length > 0 ? (
                    <ul className="space-y-1">
                      {entry.refs.map(ref => (
                        <li key={ref}>
                          <button
                            onClick={() => router.push(`/memory/${ref}`)}
                            className="text-xs text-sera-accent hover:underline transition-all font-mono"
                          >
                            {ref}
                          </button>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <p className="text-xs text-sera-text-dim italic">No references found.</p>
                  )}
                </div>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
