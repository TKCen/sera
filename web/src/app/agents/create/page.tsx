'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import {
  Bot,
  Save,
  ArrowLeft,
  Play,
  RefreshCw,
  X,
  Shield,
  MessageSquare,
  Settings as SettingsIcon,
} from 'lucide-react';
import Link from 'next/link';

interface PreviewMessage {
  role: 'user' | 'assistant';
  content: string;
}

export default function CreateAgentPage() {
  const router = useRouter();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Form State
  const [name, setName] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [role, setRole] = useState('');
  const [description, setDescription] = useState('');
  const [style, setStyle] = useState('');
  const [principles, setPrinciples] = useState('');
  const [modelName, setModelName] = useState('default');
  const [tier, setTier] = useState<1 | 2 | 3>(2);
  const [tools, setTools] = useState<string[]>(['file-read', 'file-write']);

  // Preview State
  const [showPreview, setShowPreview] = useState(false);
  const [previewMessage, setPreviewMessage] = useState('');
  const [previewHistory, setPreviewHistory] = useState<PreviewMessage[]>([]);
  const [isPreviewLoading, setIsPreviewLoading] = useState(false);

  const availableTools = [
    'file-read',
    'file-write',
    'file-list',
    'web-search',
    'web-fetch',
    'knowledge-store',
    'knowledge-query',
    'shell-exec',
    'docker-exec',
  ];

  const buildManifest = () => {
    const internalName = name.toLowerCase().replace(/[^a-z0-9]/g, '-');
    return {
      apiVersion: 'sera/v1',
      kind: 'Agent',
      metadata: {
        name: internalName,
        displayName,
        icon: '🤖',
        circle: 'general',
        tier,
      },
      identity: {
        role,
        description,
        communicationStyle: style,
        principles: principles.split('\n').filter((p) => p.trim() !== ''),
      },
      model: {
        provider: 'lm-studio',
        name: modelName,
      },
      tools: {
        allowed: tools,
      },
    };
  };

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError(null);

    try {
      const manifest = buildManifest();
      const res = await fetch(`/api/core/agents/${manifest.metadata.name}/manifest`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(manifest),
      });

      if (!res.ok) {
        const data = await res.json();
        throw new Error(data.error || 'Failed to save template');
      }

      router.push('/agents');
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  const handlePreviewChat = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!previewMessage.trim() || isPreviewLoading) return;

    const userMsg = previewMessage;
    setPreviewMessage('');
    setPreviewHistory((prev) => [...prev, { role: 'user', content: userMsg }]);
    setIsPreviewLoading(true);

    try {
      const res = await fetch('/api/core/agents/test-chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          manifest: buildManifest(),
          message: userMsg,
          history: previewHistory,
        }),
      });

      if (!res.ok) throw new Error('Preview chat failed');
      const data = await res.json();

      setPreviewHistory((prev) => [...prev, { role: 'assistant', content: data.reply }]);
    } catch (err: any) {
      setPreviewHistory((prev) => [
        ...prev,
        { role: 'assistant', content: `Error: ${err.message}` },
      ]);
    } finally {
      setIsPreviewLoading(false);
    }
  };

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="flex items-center justify-between mb-8">
        <div className="flex items-center gap-4">
          <Link
            href="/agents"
            className="p-2 text-sera-text-dim hover:text-sera-text hover:bg-sera-surface rounded-lg transition-colors"
          >
            <ArrowLeft size={20} />
          </Link>
          <div>
            <h1 className="text-2xl font-bold text-sera-text">Create Agent Template</h1>
            <p className="text-sm text-sera-text-muted">Define a new persona and capability set</p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => setShowPreview(true)}
            className="sera-btn-ghost flex items-center gap-2"
          >
            <Play size={16} />
            Test Preview
          </button>
          <button
            onClick={handleSave}
            disabled={loading || !name || !displayName}
            className="sera-btn-primary flex items-center gap-2 px-6"
          >
            {loading ? <RefreshCw size={16} className="animate-spin" /> : <Save size={16} />}
            Save Template
          </button>
        </div>
      </div>

      {error && (
        <div className="sera-card-static p-4 mb-6 border-sera-error/30 bg-sera-error/5 text-sera-error text-sm">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-8">
        {/* Left Column: Metadata & Identity */}
        <div className="space-y-6">
          <section className="sera-card-static p-6 space-y-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-sera-accent flex items-center gap-2">
              <Bot size={14} />
              Core Identity
            </h2>

            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-1.5">
                <label
                  htmlFor="agent-name"
                  className="text-[11px] font-semibold text-sera-text-dim uppercase"
                >
                  Internal Name (ID)
                </label>
                <input
                  id="agent-name"
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="e.g. creative-writer"
                  className="w-full bg-sera-surface border border-sera-border rounded px-3 py-2 text-sm focus:border-sera-accent outline-none"
                />
              </div>
              <div className="space-y-1.5">
                <label
                  htmlFor="display-name"
                  className="text-[11px] font-semibold text-sera-text-dim uppercase"
                >
                  Display Name
                </label>
                <input
                  id="display-name"
                  type="text"
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                  placeholder="e.g. Storyteller"
                  className="w-full bg-sera-surface border border-sera-border rounded px-3 py-2 text-sm focus:border-sera-accent outline-none"
                />
              </div>
            </div>

            <div className="space-y-1.5">
              <label
                htmlFor="role"
                className="text-[11px] font-semibold text-sera-text-dim uppercase"
              >
                Role Title
              </label>
              <input
                id="role"
                type="text"
                value={role}
                onChange={(e) => setRole(e.target.value)}
                placeholder="e.g. Senior Creative Writer & Editor"
                className="w-full bg-sera-surface border border-sera-border rounded px-3 py-2 text-sm focus:border-sera-accent outline-none"
              />
            </div>

            <div className="space-y-1.5">
              <label
                htmlFor="description"
                className="text-[11px] font-semibold text-sera-text-dim uppercase"
              >
                Description / Persona
              </label>
              <textarea
                id="description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                rows={3}
                placeholder="Describe the agent's background and purpose..."
                className="w-full bg-sera-surface border border-sera-border rounded px-3 py-2 text-sm focus:border-sera-accent outline-none resize-none"
              />
            </div>
          </section>

          <section className="sera-card-static p-6 space-y-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-sera-accent flex items-center gap-2">
              <MessageSquare size={14} />
              Communication
            </h2>

            <div className="space-y-1.5">
              <label
                htmlFor="style"
                className="text-[11px] font-semibold text-sera-text-dim uppercase"
              >
                Style Guidelines
              </label>
              <input
                id="style"
                type="text"
                value={style}
                onChange={(e) => setStyle(e.target.value)}
                placeholder="e.g. Formal, technical, and concise"
                className="w-full bg-sera-surface border border-sera-border rounded px-3 py-2 text-sm focus:border-sera-accent outline-none"
              />
            </div>

            <div className="space-y-1.5">
              <label
                htmlFor="principles"
                className="text-[11px] font-semibold text-sera-text-dim uppercase"
              >
                System Prompt / Principles (One per line)
              </label>
              <textarea
                id="principles"
                value={principles}
                onChange={(e) => setPrinciples(e.target.value)}
                rows={4}
                placeholder="Always double-check facts&#10;Prioritize brevity&#10;Use markdown for structure"
                className="w-full bg-sera-surface border border-sera-border rounded px-3 py-2 text-sm focus:border-sera-accent outline-none resize-none"
              />
            </div>
          </section>
        </div>

        {/* Right Column: Capabilities & Security */}
        <div className="space-y-6">
          <section className="sera-card-static p-6 space-y-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-sera-accent flex items-center gap-2">
              <SettingsIcon size={14} />
              Model Configuration
            </h2>
            <div className="space-y-1.5">
              <label
                htmlFor="model-name"
                className="text-[11px] font-semibold text-sera-text-dim uppercase"
              >
                Model Name (LM-Studio)
              </label>
              <input
                id="model-name"
                type="text"
                value={modelName}
                onChange={(e) => setModelName(e.target.value)}
                placeholder="e.g. default"
                className="w-full bg-sera-surface border border-sera-border rounded px-3 py-2 text-sm focus:border-sera-accent outline-none"
              />
            </div>
          </section>

          <section className="sera-card-static p-6 space-y-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-sera-accent flex items-center gap-2">
              <Shield size={14} />
              Security & Tier
            </h2>

            <div className="flex gap-2">
              {[1, 2, 3].map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setTier(t as any)}
                  className={`flex-1 py-2 rounded border text-xs font-bold transition-all ${
                    tier === t
                      ? 'bg-sera-accent/10 border-sera-accent text-sera-accent'
                      : 'bg-sera-surface border-sera-border text-sera-text-dim hover:border-sera-border-hover'
                  }`}
                >
                  Tier {t}
                </button>
              ))}
            </div>
            <p className="text-[10px] text-sera-text-muted leading-relaxed">
              Tier 1: Restricted. Tier 2: Standard (Recommended). Tier 3: High Privilege.
            </p>
          </section>

          <section className="sera-card-static p-6 space-y-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-sera-accent">
              Allowed Tools
            </h2>
            <div className="grid grid-cols-2 gap-2">
              {availableTools.map((tool) => (
                <label
                  key={tool}
                  className="flex items-center gap-2 p-2 rounded hover:bg-sera-surface cursor-pointer group"
                >
                  <input
                    type="checkbox"
                    checked={tools.includes(tool)}
                    onChange={(e) => {
                      if (e.target.checked) setTools([...tools, tool]);
                      else setTools(tools.filter((t) => t !== tool));
                    }}
                    className="rounded border-sera-border text-sera-accent focus:ring-sera-accent bg-sera-surface"
                  />
                  <span className="text-xs text-sera-text-muted group-hover:text-sera-text transition-colors">
                    {tool}
                  </span>
                </label>
              ))}
            </div>
          </section>
        </div>
      </div>

      {/* Preview Modal */}
      {showPreview && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-8 bg-sera-bg/90 backdrop-blur-md">
          <div className="sera-card-static w-full max-w-2xl h-[600px] flex flex-col shadow-2xl border-sera-accent/30 animate-in fade-in zoom-in-95 duration-300">
            <div className="flex items-center justify-between p-4 border-b border-sera-border">
              <div className="flex items-center gap-2">
                <div className="w-8 h-8 rounded-lg bg-sera-accent/10 flex items-center justify-center text-sera-accent">
                  <Play size={16} />
                </div>
                <div>
                  <h3 className="text-sm font-bold text-sera-text">
                    Persona Preview: {displayName || 'Unnamed Agent'}
                  </h3>
                  <p className="text-[10px] text-sera-text-dim uppercase tracking-widest">
                    Transient Simulation Mode
                  </p>
                </div>
              </div>
              <button
                onClick={() => setShowPreview(false)}
                className="p-1 hover:bg-sera-surface rounded"
              >
                <X size={18} className="text-sera-text-dim" />
              </button>
            </div>

            <div className="flex-1 overflow-y-auto p-6 space-y-6">
              {previewHistory.length === 0 && (
                <div className="h-full flex flex-col items-center justify-center text-center space-y-3 opacity-50">
                  <MessageSquare size={40} className="text-sera-accent" />
                  <p className="text-sm text-sera-text-muted">
                    Send a message to test how your agent <br />
                    responds with the current configuration.
                  </p>
                </div>
              )}
              {previewHistory.map((msg, i) => (
                <div
                  key={i}
                  className={`flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}
                >
                  <div
                    className={`max-w-[85%] p-4 rounded-xl text-sm ${
                      msg.role === 'user'
                        ? 'bg-sera-accent text-white'
                        : 'bg-sera-surface border border-sera-border text-sera-text'
                    }`}
                  >
                    {msg.content}
                  </div>
                </div>
              ))}
              {isPreviewLoading && (
                <div className="flex justify-start">
                  <div className="bg-sera-surface border border-sera-border p-4 rounded-xl flex items-center gap-2">
                    <RefreshCw size={14} className="animate-spin text-sera-accent" />
                    <span className="text-xs text-sera-text-muted">Agent is thinking...</span>
                  </div>
                </div>
              )}
            </div>

            <form
              onSubmit={handlePreviewChat}
              className="p-4 bg-sera-surface border-t border-sera-border"
            >
              <div className="relative">
                <input
                  type="text"
                  value={previewMessage}
                  onChange={(e) => setPreviewMessage(e.target.value)}
                  placeholder="Test the agent's persona..."
                  className="w-full bg-sera-bg border border-sera-border rounded-lg pl-4 pr-12 py-3 text-sm focus:border-sera-accent outline-none"
                />
                <button
                  type="submit"
                  disabled={!previewMessage.trim() || isPreviewLoading}
                  className="absolute right-2 top-2 p-2 text-sera-accent hover:bg-sera-accent/10 rounded-lg disabled:opacity-50 transition-colors"
                >
                  <Play size={18} />
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
