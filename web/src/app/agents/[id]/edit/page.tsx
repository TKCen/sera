'use client';

import { useParams } from 'next/navigation';
import { useState, useEffect } from 'react';
import Link from 'next/link';
import { ArrowLeft, Save, Code, LayoutDashboard } from 'lucide-react';
import yaml from 'js-yaml';

// Define the types that correspond to the manifest
interface AgentManifest {
  apiVersion: string;
  kind: string;
  metadata: {
    name: string;
    displayName: string;
    icon: string;
    circle: string;
    tier: number;
    [key: string]: unknown;
  };
  identity: {
    role: string;
    description: string;
    communicationStyle?: string;
    principles?: string[];
  };
  model: {
    provider: string;
    name: string;
    temperature?: number;
    fallback?: Array<{ provider: string; name: string; maxComplexity?: number }>;
  };
  tools?: { allowed?: string[]; denied?: string[] };
  skills?: string[];
  subagents?: {
    allowed?: Array<{ role: string; maxInstances?: number; requiresApproval?: boolean }>;
  };
  intercom?: {
    canMessage?: string[];
    channels?: { publish?: string[]; subscribe?: string[] };
  };
  resources?: { memory?: string; cpu?: string };
  workspace?: { provider?: string; path?: string };
  memory?: { personalMemory?: string; sharedKnowledge?: string };
}

export default function AgentEditPage() {
  const params = useParams();
  const agentName = params.id as string;

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const [rawMode, setRawMode] = useState(false);
  const [rawYaml, setRawYaml] = useState<string>('');

  const [manifest, setManifest] = useState<AgentManifest | null>(null);
  const [availableSkills, setAvailableSkills] = useState<{ id: string; name: string }[]>([]);

  const handleManifestChange = (path: string[], value: unknown) => {
    if (!manifest) return;

    setManifest((prev) => {
      if (!prev) return prev;

      // Deep clone the object to ensure immutability before modifying
      const updated = JSON.parse(JSON.stringify(prev));
      let current: Record<string, unknown> = updated as unknown as Record<string, unknown>;

      for (let i = 0; i < path.length - 1; i++) {
        const key = path[i];
        if (!current[key]) {
          current[key] = {};
        }
        current = current[key] as Record<string, unknown>;
      }

      current[path[path.length - 1]] = value;
      return updated;
    });
  };

  const updatePrinciples = (index: number, value: string) => {
    if (!manifest?.identity?.principles) return;
    const newPrinciples = [...manifest.identity.principles];
    newPrinciples[index] = value;
    handleManifestChange(['identity', 'principles'], newPrinciples);
  };

  const addPrinciple = () => {
    const principles = manifest?.identity?.principles || [];
    handleManifestChange(['identity', 'principles'], [...principles, '']);
  };

  const removePrinciple = (index: number) => {
    if (!manifest?.identity?.principles) return;
    const newPrinciples = manifest.identity.principles.filter((_, i) => i !== index);
    handleManifestChange(['identity', 'principles'], newPrinciples);
  };

  const validateForm = (): boolean => {
    if (!manifest) return false;

    // Required fields
    if (!manifest.metadata?.name) {
      setError('Metadata Name is required.');
      return false;
    }
    if (!manifest.identity?.role) {
      setError('Identity Role is required.');
      return false;
    }
    if (!manifest.model?.provider) {
      setError('Model Provider is required.');
      return false;
    }
    if (!manifest.model?.name) {
      setError('Model Name is required.');
      return false;
    }

    // Numeric validation for temperature (0-2)
    if (manifest.model.temperature !== undefined) {
      if (manifest.model.temperature < 0 || manifest.model.temperature > 2) {
        setError('Temperature must be between 0 and 2.');
        return false;
      }
    }

    // Tier must be 1, 2, or 3
    if (![1, 2, 3].includes(manifest.metadata?.tier)) {
      setError('Security Tier must be 1, 2, or 3.');
      return false;
    }

    // Resource validation
    if (manifest.resources?.cpu && isNaN(Number(manifest.resources.cpu))) {
      setError('CPU Limit must be a numeric value.');
      return false;
    }

    if (manifest.resources?.memory && !/^\d+(Mi|Gi|M|G)$/.test(manifest.resources.memory)) {
      setError('Memory Limit must be a valid resource string (e.g., 512Mi, 1Gi).');
      return false;
    }

    return true;
  };

  const toggleTool = (toolId: string, listType: 'allowed' | 'denied') => {
    const currentList = manifest?.tools?.[listType] || [];
    const newList = currentList.includes(toolId)
      ? currentList.filter((t) => t !== toolId)
      : [...currentList, toolId];

    // If adding to one list, remove from the other to prevent conflicts
    const otherListType = listType === 'allowed' ? 'denied' : 'allowed';
    let otherList = manifest?.tools?.[otherListType] || [];
    if (newList.includes(toolId) && otherList.includes(toolId)) {
      otherList = otherList.filter((t) => t !== toolId);
      handleManifestChange(['tools', otherListType], otherList);
    }

    handleManifestChange(['tools', listType], newList);
  };

  // Fetch the agent manifest
  useEffect(() => {
    Promise.all([fetch(`/api/core/agents/${agentName}`), fetch(`/api/core/skills`)])
      .then(async ([agentRes, skillsRes]) => {
        if (!agentRes.ok) throw new Error(`Agent not found`);
        const agentData = await agentRes.json();
        setManifest(agentData.manifest);
        setRawYaml(
          yaml.dump(agentData.manifest, { lineWidth: 120, noRefs: true, sortKeys: false })
        );

        if (skillsRes.ok) {
          const skillsData = await skillsRes.json();
          setAvailableSkills(skillsData);
        }
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [agentName]);

  const handleSave = async () => {
    setError(null);
    setSuccess(null);

    // If in raw mode, parse it first to ensure it's valid and get the latest JSON
    let payload = manifest;
    if (rawMode) {
      try {
        payload = yaml.load(rawYaml) as AgentManifest;
        setManifest(payload); // Update the visual state too
      } catch (err: unknown) {
        const errorMessage = err instanceof Error ? err.message : String(err);
        setError(`Cannot save due to YAML syntax error: ${errorMessage}`);
        return;
      }
    } else {
      if (!validateForm()) {
        return; // validateForm will set the error message
      }
    }

    setSaving(true);
    try {
      // 1. Save manifest
      const res = await fetch(`/api/core/agents/${agentName}/manifest`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });

      if (!res.ok) {
        const data = await res.json();
        throw new Error(data.error || 'Failed to save manifest');
      }

      // 2. Trigger reload
      const reloadRes = await fetch(`/api/core/agents/reload`, {
        method: 'POST',
      });

      if (!reloadRes.ok) {
        throw new Error('Manifest saved, but failed to trigger hot reload');
      }

      setSuccess('Manifest saved successfully.');
      setTimeout(() => setSuccess(null), 3000);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const toggleRawMode = () => {
    if (rawMode) {
      // Trying to switch to visual mode, parse the YAML
      try {
        const parsed = yaml.load(rawYaml) as AgentManifest;
        setManifest(parsed);
        setRawMode(false);
        setError(null);
      } catch (err: unknown) {
        const errorMessage = err instanceof Error ? err.message : String(err);
        setError(`YAML Parsing Error: ${errorMessage}`);
      }
    } else {
      // Trying to switch to raw mode, dump the manifest
      if (manifest) {
        setRawYaml(yaml.dump(manifest, { lineWidth: 120, noRefs: true, sortKeys: false }));
      }
      setRawMode(true);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <span className="text-sm text-sera-text-muted">Loading…</span>
      </div>
    );
  }

  if (error && !manifest && !rawMode) {
    return (
      <div className="p-8 max-w-5xl mx-auto">
        <Link
          href={`/agents/${agentName}`}
          className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4"
        >
          <ArrowLeft size={14} /> Back to Agent
        </Link>
        <div className="sera-card-static p-6 text-center">
          <p className="text-sm text-sera-error">{error}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Breadcrumb */}
      <Link
        href={`/agents/${agentName}`}
        className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4"
      >
        <ArrowLeft size={14} /> Back to Agent
      </Link>

      {/* Header */}
      <div className="flex items-start justify-between mb-8">
        <div>
          <h1 className="sera-page-title">Edit {manifest?.metadata?.displayName || agentName}</h1>
          <p className="text-sm text-sera-text-muted mt-0.5">
            Modify the manifest configuration for this agent.
          </p>
        </div>

        <div className="flex items-center gap-3">
          <button onClick={toggleRawMode} className="sera-btn-ghost" type="button">
            {rawMode ? (
              <>
                <LayoutDashboard size={16} /> Visual Editor
              </>
            ) : (
              <>
                <Code size={16} /> Raw YAML
              </>
            )}
          </button>

          <button onClick={handleSave} disabled={saving} className="sera-btn-primary" type="button">
            <Save size={16} />
            {saving ? 'Saving...' : 'Save Manifest'}
          </button>
        </div>
      </div>

      {error && (
        <div className="mb-6 p-4 rounded-lg border border-sera-error/30 bg-sera-error/10 text-sera-error text-sm">
          {error}
        </div>
      )}

      {success && (
        <div className="mb-6 p-4 rounded-lg border border-sera-success/30 bg-sera-success/10 text-sera-success text-sm">
          {success}
        </div>
      )}

      {/* Main Content Area */}
      <div className="sera-card-static p-6">
        {rawMode ? (
          <div>
            <textarea
              className="sera-input font-mono text-sm min-h-[500px]"
              value={rawYaml}
              onChange={(e) => setRawYaml(e.target.value)}
              spellCheck={false}
            />
          </div>
        ) : (
          <div className="space-y-8">
            {/* Identity Section */}
            <div className="border border-sera-border rounded-xl p-5">
              <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4 border-b border-sera-border pb-2">
                Identity
              </h3>
              <div className="grid grid-cols-1 gap-4">
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">
                    Role <span className="text-sera-error">*</span>
                  </label>
                  <input
                    type="text"
                    className="sera-input"
                    value={manifest?.identity?.role || ''}
                    onChange={(e) => handleManifestChange(['identity', 'role'], e.target.value)}
                    required
                  />
                </div>
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">Description</label>
                  <textarea
                    className="sera-input min-h-[80px]"
                    value={manifest?.identity?.description || ''}
                    onChange={(e) =>
                      handleManifestChange(['identity', 'description'], e.target.value)
                    }
                  />
                </div>
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">
                    Communication Style
                  </label>
                  <textarea
                    className="sera-input min-h-[80px]"
                    value={manifest?.identity?.communicationStyle || ''}
                    onChange={(e) =>
                      handleManifestChange(['identity', 'communicationStyle'], e.target.value)
                    }
                  />
                </div>
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5 flex justify-between items-center">
                    <span>Principles</span>
                    <button
                      type="button"
                      onClick={addPrinciple}
                      className="text-sera-accent hover:text-white transition-colors text-[10px] uppercase tracking-wider px-2 py-1 bg-sera-accent/10 rounded"
                    >
                      + Add Principle
                    </button>
                  </label>
                  <div className="space-y-2">
                    {manifest?.identity?.principles?.map((principle, idx) => (
                      <div key={idx} className="flex gap-2">
                        <input
                          type="text"
                          className="sera-input"
                          value={principle}
                          onChange={(e) => updatePrinciples(idx, e.target.value)}
                        />
                        <button
                          type="button"
                          onClick={() => removePrinciple(idx)}
                          className="text-sera-error hover:bg-sera-error/10 px-3 rounded-lg border border-sera-border"
                        >
                          ✕
                        </button>
                      </div>
                    ))}
                    {(!manifest?.identity?.principles ||
                      manifest.identity.principles.length === 0) && (
                      <div className="text-xs text-sera-text-dim italic">
                        No principles defined.
                      </div>
                    )}
                  </div>
                </div>
              </div>
            </div>

            {/* Model Section */}
            <div className="border border-sera-border rounded-xl p-5">
              <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4 border-b border-sera-border pb-2">
                Model
              </h3>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">
                    Provider <span className="text-sera-error">*</span>
                  </label>
                  <select
                    className="sera-input"
                    value={manifest?.model?.provider || ''}
                    onChange={(e) => handleManifestChange(['model', 'provider'], e.target.value)}
                    required
                  >
                    <option value="">Select a provider...</option>
                    <option value="openai">OpenAI</option>
                    <option value="anthropic">Anthropic</option>
                    <option value="lm-studio">LM Studio</option>
                    <option value="ollama">Ollama</option>
                  </select>
                </div>
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">
                    Model Name <span className="text-sera-error">*</span>
                  </label>
                  <input
                    type="text"
                    className="sera-input"
                    value={manifest?.model?.name || ''}
                    onChange={(e) => handleManifestChange(['model', 'name'], e.target.value)}
                    required
                  />
                </div>
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">Temperature</label>
                  <div className="flex gap-4 items-center">
                    <input
                      type="range"
                      min="0"
                      max="2"
                      step="0.1"
                      className="flex-grow accent-sera-accent"
                      value={manifest?.model?.temperature ?? 0.7}
                      onChange={(e) =>
                        handleManifestChange(['model', 'temperature'], parseFloat(e.target.value))
                      }
                    />
                    <span className="w-10 text-right text-sm text-sera-text-dim font-mono">
                      {manifest?.model?.temperature ?? 0.7}
                    </span>
                  </div>
                </div>
                <div className="md:col-span-2">
                  <label className="block text-xs text-sera-text-muted mb-1.5">
                    Fallback Models
                  </label>
                  <div className="space-y-2 border border-sera-border/50 rounded-lg p-3 bg-sera-bg/50">
                    {manifest?.model?.fallback?.map((fb, idx) => (
                      <div key={idx} className="flex flex-wrap gap-2 items-center">
                        <input
                          type="text"
                          placeholder="Provider"
                          className="sera-input flex-1 min-w-[100px] text-xs py-1.5"
                          value={fb.provider || ''}
                          onChange={(e) => {
                            const newFallback = [...(manifest.model.fallback || [])];
                            newFallback[idx] = { ...newFallback[idx], provider: e.target.value };
                            handleManifestChange(['model', 'fallback'], newFallback);
                          }}
                        />
                        <input
                          type="text"
                          placeholder="Name"
                          className="sera-input flex-1 min-w-[120px] text-xs py-1.5"
                          value={fb.name || ''}
                          onChange={(e) => {
                            const newFallback = [...(manifest.model.fallback || [])];
                            newFallback[idx] = { ...newFallback[idx], name: e.target.value };
                            handleManifestChange(['model', 'fallback'], newFallback);
                          }}
                        />
                        <input
                          type="number"
                          placeholder="Max Cplx"
                          className="sera-input w-24 text-xs py-1.5"
                          value={fb.maxComplexity || ''}
                          onChange={(e) => {
                            const newFallback = [...(manifest.model.fallback || [])];
                            newFallback[idx] = {
                              ...newFallback[idx],
                              maxComplexity: parseInt(e.target.value) || undefined,
                            };
                            handleManifestChange(['model', 'fallback'], newFallback);
                          }}
                        />
                        <button
                          type="button"
                          className="text-sera-error hover:bg-sera-error/10 px-2 py-1 rounded-md border border-sera-border text-xs"
                          onClick={() => {
                            const newFallback = manifest.model.fallback?.filter(
                              (_, i) => i !== idx
                            );
                            handleManifestChange(['model', 'fallback'], newFallback);
                          }}
                        >
                          ✕
                        </button>
                      </div>
                    ))}
                    <button
                      type="button"
                      onClick={() => {
                        const fallbacks = manifest?.model?.fallback || [];
                        handleManifestChange(
                          ['model', 'fallback'],
                          [...fallbacks, { provider: '', name: '' }]
                        );
                      }}
                      className="text-sera-accent hover:text-white transition-colors text-[10px] uppercase tracking-wider px-2 py-1 bg-sera-accent/10 rounded mt-1"
                    >
                      + Add Fallback
                    </button>
                  </div>
                </div>
              </div>
            </div>

            {/* Tools Section */}
            <div className="border border-sera-border rounded-xl p-5">
              <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4 border-b border-sera-border pb-2">
                Tools
              </h3>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                <div>
                  <label className="block text-xs font-medium text-sera-text mb-3">
                    Allowed Tools
                  </label>
                  <div className="space-y-2 max-h-60 overflow-y-auto pr-2">
                    {availableSkills.map((skill) => (
                      <label
                        key={`allowed-${skill.id}`}
                        className="flex items-start gap-2 cursor-pointer group"
                      >
                        <input
                          type="checkbox"
                          className="mt-1 accent-sera-accent rounded border-sera-border bg-sera-bg"
                          checked={(manifest?.tools?.allowed || []).includes(skill.id)}
                          onChange={() => toggleTool(skill.id, 'allowed')}
                        />
                        <div>
                          <span className="text-sm text-sera-text group-hover:text-sera-accent transition-colors">
                            {skill.id}
                          </span>
                          {skill.name && (
                            <p className="text-[10px] text-sera-text-dim">{skill.name}</p>
                          )}
                        </div>
                      </label>
                    ))}
                    {availableSkills.length === 0 && (
                      <span className="text-xs text-sera-text-dim italic">No tools available</span>
                    )}
                  </div>
                </div>
                <div>
                  <label className="block text-xs font-medium text-sera-text mb-3">
                    Denied Tools
                  </label>
                  <div className="space-y-2 max-h-60 overflow-y-auto pr-2">
                    {availableSkills.map((skill) => (
                      <label
                        key={`denied-${skill.id}`}
                        className="flex items-start gap-2 cursor-pointer group"
                      >
                        <input
                          type="checkbox"
                          className="mt-1 accent-sera-error rounded border-sera-border bg-sera-bg"
                          checked={(manifest?.tools?.denied || []).includes(skill.id)}
                          onChange={() => toggleTool(skill.id, 'denied')}
                        />
                        <div>
                          <span className="text-sm text-sera-text group-hover:text-sera-error transition-colors">
                            {skill.id}
                          </span>
                          {skill.name && (
                            <p className="text-[10px] text-sera-text-dim">{skill.name}</p>
                          )}
                        </div>
                      </label>
                    ))}
                    {availableSkills.length === 0 && (
                      <span className="text-xs text-sera-text-dim italic">No tools available</span>
                    )}
                  </div>
                </div>
              </div>
            </div>

            {/* Subagents Section */}
            <div className="border border-sera-border rounded-xl p-5">
              <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4 border-b border-sera-border pb-2 flex justify-between items-center">
                <span>Subagents</span>
                <button
                  type="button"
                  onClick={() => {
                    const subagents = manifest?.subagents?.allowed || [];
                    handleManifestChange(
                      ['subagents', 'allowed'],
                      [...subagents, { role: '', maxInstances: 1 }]
                    );
                  }}
                  className="text-sera-accent hover:text-white transition-colors text-[10px] uppercase tracking-wider px-2 py-1 bg-sera-accent/10 rounded"
                >
                  + Add Subagent
                </button>
              </h3>
              <div className="space-y-3">
                {manifest?.subagents?.allowed?.map((subagent, idx) => (
                  <div
                    key={idx}
                    className="flex flex-wrap gap-4 items-center bg-sera-bg/50 p-3 rounded-lg border border-sera-border/50"
                  >
                    <div className="flex-1 min-w-[200px]">
                      <label className="block text-[10px] text-sera-text-muted mb-1 uppercase tracking-wide">
                        Role <span className="text-sera-error">*</span>
                      </label>
                      <input
                        type="text"
                        className="sera-input"
                        placeholder="e.g. researcher"
                        value={subagent.role || ''}
                        onChange={(e) => {
                          const newSubagents = [...(manifest.subagents?.allowed || [])];
                          newSubagents[idx] = { ...newSubagents[idx], role: e.target.value };
                          handleManifestChange(['subagents', 'allowed'], newSubagents);
                        }}
                        required
                      />
                    </div>
                    <div className="w-24">
                      <label className="block text-[10px] text-sera-text-muted mb-1 uppercase tracking-wide">
                        Max Instances
                      </label>
                      <input
                        type="number"
                        min="1"
                        className="sera-input"
                        value={subagent.maxInstances || ''}
                        onChange={(e) => {
                          const newSubagents = [...(manifest.subagents?.allowed || [])];
                          newSubagents[idx] = {
                            ...newSubagents[idx],
                            maxInstances: parseInt(e.target.value) || undefined,
                          };
                          handleManifestChange(['subagents', 'allowed'], newSubagents);
                        }}
                      />
                    </div>
                    <div className="flex items-center gap-2 mt-4">
                      <label className="flex items-center gap-2 text-xs text-sera-text-muted cursor-pointer">
                        <input
                          type="checkbox"
                          className="accent-sera-warning"
                          checked={subagent.requiresApproval || false}
                          onChange={(e) => {
                            const newSubagents = [...(manifest.subagents?.allowed || [])];
                            newSubagents[idx] = {
                              ...newSubagents[idx],
                              requiresApproval: e.target.checked,
                            };
                            handleManifestChange(['subagents', 'allowed'], newSubagents);
                          }}
                        />
                        Requires Approval
                      </label>
                    </div>
                    <button
                      type="button"
                      className="text-sera-error hover:bg-sera-error/10 p-2 rounded-md border border-sera-border mt-4"
                      onClick={() => {
                        const newSubagents = manifest.subagents?.allowed?.filter(
                          (_, i) => i !== idx
                        );
                        handleManifestChange(['subagents', 'allowed'], newSubagents);
                      }}
                    >
                      ✕
                    </button>
                  </div>
                ))}
                {(!manifest?.subagents?.allowed || manifest.subagents.allowed.length === 0) && (
                  <div className="text-xs text-sera-text-dim italic text-center py-4 bg-sera-bg/30 rounded-lg border border-dashed border-sera-border">
                    No subagents configured.
                  </div>
                )}
              </div>
            </div>

            {/* Resources Section */}
            <div className="border border-sera-border rounded-xl p-5">
              <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4 border-b border-sera-border pb-2">
                Resources & Security
              </h3>
              <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">
                    Security Tier <span className="text-sera-error">*</span>
                  </label>
                  <select
                    className="sera-input"
                    value={manifest?.metadata?.tier || 1}
                    onChange={(e) =>
                      handleManifestChange(['metadata', 'tier'], parseInt(e.target.value))
                    }
                    required
                  >
                    <option value={1}>Tier 1 (Restricted)</option>
                    <option value={2}>Tier 2 (Standard)</option>
                    <option value={3}>Tier 3 (Privileged)</option>
                  </select>
                </div>
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">Memory Limit</label>
                  <input
                    type="text"
                    className="sera-input"
                    placeholder="e.g. 512Mi, 1Gi"
                    value={manifest?.resources?.memory || ''}
                    onChange={(e) => handleManifestChange(['resources', 'memory'], e.target.value)}
                  />
                </div>
                <div>
                  <label className="block text-xs text-sera-text-muted mb-1.5">CPU Limit</label>
                  <input
                    type="text"
                    className="sera-input"
                    placeholder="e.g. 0.5, 1.0"
                    value={manifest?.resources?.cpu || ''}
                    onChange={(e) => handleManifestChange(['resources', 'cpu'], e.target.value)}
                  />
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
