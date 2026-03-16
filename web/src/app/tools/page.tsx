'use client';

import { useState, useEffect } from 'react';
import { Wrench, Bot, RefreshCw, Package, Plug, Code } from 'lucide-react';

interface SkillInfo {
  id: string;
  description: string;
  source: 'builtin' | 'mcp' | 'custom';
  parameters: Array<{ name: string; type: string; description: string; required: boolean }>;
  usedBy: string[];
}

const SOURCE_STYLES: Record<string, { label: string; class: string; icon: React.ReactNode }> = {
  builtin: { label: 'Built-in', class: 'bg-emerald-500/15 text-emerald-400', icon: <Package size={10} /> },
  mcp: { label: 'MCP', class: 'bg-blue-500/15 text-blue-400', icon: <Plug size={10} /> },
  custom: { label: 'Custom', class: 'bg-purple-500/15 text-purple-400', icon: <Code size={10} /> },
};

export default function ToolsPage() {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedSkill, setExpandedSkill] = useState<string | null>(null);

  useEffect(() => {
    fetch('/api/core/skills')
      .then(async (res) => {
        if (!res.ok) throw new Error('Failed to fetch skills');
        return res.json();
      })
      .then(setSkills)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, []);

  // Group by source
  const grouped = skills.reduce<Record<string, SkillInfo[]>>((acc, skill) => {
    const group = skill.source || 'custom';
    if (!acc[group]) acc[group] = [];
    acc[group].push(skill);
    return acc;
  }, {});

  return (
    <div className="p-8 max-w-7xl mx-auto">
      {/* Header */}
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Tools & Skills</h1>
          <p className="text-sm text-sera-text-muted mt-1">Browse and manage agent capabilities</p>
        </div>
      </div>

      {/* Loading */}
      {loading && (
        <div className="flex items-center justify-center py-20">
          <div className="flex items-center gap-3 text-sera-text-muted">
            <RefreshCw size={18} className="animate-spin" />
            <span className="text-sm">Loading skills…</span>
          </div>
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="sera-card-static p-4 mb-6 border-sera-error/30 bg-sera-error/5">
          <p className="text-sm text-sera-error">{error}</p>
        </div>
      )}

      {/* Summary Cards */}
      {!loading && skills.length > 0 && (
        <div className="grid grid-cols-3 gap-4 mb-8">
          {(['builtin', 'mcp', 'custom'] as const).map((source) => {
            const count = grouped[source]?.length ?? 0;
            const style = SOURCE_STYLES[source];
            return (
              <div key={source} className="sera-card-static p-4">
                <div className="flex items-center gap-2 mb-2">
                  <span className={`sera-badge ${style.class}`}>
                    {style.icon}
                    <span className="ml-1">{style.label}</span>
                  </span>
                </div>
                <p className="text-2xl font-semibold text-sera-text">{count}</p>
                <p className="text-[11px] text-sera-text-dim">skill{count !== 1 ? 's' : ''} registered</p>
              </div>
            );
          })}
        </div>
      )}

      {/* Skills List */}
      {!loading && Object.entries(grouped).map(([source, sourceSkills]) => (
        <section key={source} className="mb-8">
          <h2 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
            {SOURCE_STYLES[source]?.label || source} Skills ({sourceSkills.length})
          </h2>
          <div className="space-y-2">
            {sourceSkills.map((skill) => (
              <div key={skill.id} className="sera-card-static rounded-lg">
                <button
                  className="w-full p-4 text-left"
                  onClick={() => setExpandedSkill(expandedSkill === skill.id ? null : skill.id)}
                >
                  <div className="flex items-start justify-between">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <Wrench size={14} className="text-sera-text-dim flex-shrink-0" />
                        <h3 className="text-sm font-medium text-sera-text">{skill.id}</h3>
                        <span className={`sera-badge ${SOURCE_STYLES[skill.source]?.class}`}>
                          {SOURCE_STYLES[skill.source]?.icon}
                          <span className="ml-0.5">{SOURCE_STYLES[skill.source]?.label}</span>
                        </span>
                      </div>
                      <p className="text-xs text-sera-text-muted mt-1 line-clamp-1">{skill.description}</p>
                    </div>
                    {skill.usedBy.length > 0 && (
                      <div className="flex items-center gap-1 ml-4 flex-shrink-0">
                        <Bot size={12} className="text-sera-text-dim" />
                        <span className="text-[11px] text-sera-text-dim">{skill.usedBy.length}</span>
                      </div>
                    )}
                  </div>
                </button>

                {/* Expanded Detail */}
                {expandedSkill === skill.id && (
                  <div className="px-4 pb-4 space-y-3 border-t border-sera-border pt-3">
                    <p className="text-xs text-sera-text-muted">{skill.description}</p>

                    {/* Parameters */}
                    {skill.parameters.length > 0 && (
                      <div>
                        <h4 className="text-[11px] text-sera-text-dim uppercase tracking-wide mb-2">Parameters</h4>
                        <div className="space-y-1.5">
                          {skill.parameters.map((param) => (
                            <div key={param.name} className="flex items-start gap-2">
                              <code className="text-[11px] text-sera-accent font-mono bg-sera-accent-soft px-1.5 py-0.5 rounded">
                                {param.name}
                              </code>
                              <span className="text-[11px] text-sera-text-dim">{param.type}</span>
                              {param.required && <span className="text-[11px] text-sera-error">required</span>}
                              {param.description && (
                                <span className="text-[11px] text-sera-text-muted">— {param.description}</span>
                              )}
                            </div>
                          ))}
                        </div>
                      </div>
                    )}

                    {/* Used By */}
                    {skill.usedBy.length > 0 && (
                      <div>
                        <h4 className="text-[11px] text-sera-text-dim uppercase tracking-wide mb-2">Used By Agents</h4>
                        <div className="flex flex-wrap gap-1.5">
                          {skill.usedBy.map((agent) => (
                            <span key={agent} className="sera-badge-accent">{agent}</span>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            ))}
          </div>
        </section>
      ))}

      {/* Empty State */}
      {!loading && skills.length === 0 && !error && (
        <div className="flex flex-col items-center justify-center py-20">
          <div className="w-14 h-14 rounded-xl bg-sera-surface flex items-center justify-center mb-4">
            <Wrench size={28} className="text-sera-text-dim" />
          </div>
          <h3 className="text-sm font-semibold text-sera-text mb-1">No skills registered</h3>
          <p className="text-xs text-sera-text-muted text-center max-w-sm">
            Skills will appear here once agents and MCP servers are configured.
          </p>
        </div>
      )}
    </div>
  );
}
