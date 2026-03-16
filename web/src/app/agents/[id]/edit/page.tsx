'use client';

import { useParams, useRouter } from 'next/navigation';
import { useState, useEffect } from 'react';
import { ArrowLeft, Save, Code, FormInput, RefreshCw } from 'lucide-react';
import Link from 'next/link';

type EditorMode = 'form' | 'yaml';

export default function AgentEditPage() {
  const params = useParams();
  const router = useRouter();
  const agentName = params.id as string;

  const [manifest, setManifest] = useState<any>(null);
  const [rawYaml, setRawYaml] = useState('');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [mode, setMode] = useState<EditorMode>('form');

  // Form state
  const [formData, setFormData] = useState({
    displayName: '',
    icon: '',
    circle: '',
    tier: 1 as number,
    role: '',
    description: '',
    communicationStyle: '',
    principles: [] as string[],
    modelProvider: '',
    modelName: '',
    temperature: 0.7,
    toolsAllowed: [] as string[],
    toolsDenied: [] as string[],
    skills: [] as string[],
    memoryLimit: '',
    cpuLimit: '',
  });

  useEffect(() => {
    Promise.all([
      fetch(`/api/core/agents/${agentName}`).then(r => r.json()),
      fetch(`/api/core/agents/${agentName}/manifest/raw`).then(r => r.text()),
    ])
      .then(([detail, yaml]) => {
        setManifest(detail.manifest);
        setRawYaml(yaml);
        const m = detail.manifest;
        setFormData({
          displayName: m.metadata.displayName || '',
          icon: m.metadata.icon || '',
          circle: m.metadata.circle || '',
          tier: m.metadata.tier || 1,
          role: m.identity.role || '',
          description: m.identity.description || '',
          communicationStyle: m.identity.communicationStyle || '',
          principles: m.identity.principles || [],
          modelProvider: m.model.provider || '',
          modelName: m.model.name || '',
          temperature: m.model.temperature ?? 0.7,
          toolsAllowed: m.tools?.allowed || [],
          toolsDenied: m.tools?.denied || [],
          skills: m.skills || [],
          memoryLimit: m.resources?.memory || '',
          cpuLimit: m.resources?.cpu || '',
        });
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [agentName]);

  const handleFormSave = async () => {
    if (!manifest) return;
    setSaving(true);
    setError(null);
    setSuccess(false);

    const updated = {
      ...manifest,
      metadata: {
        ...manifest.metadata,
        displayName: formData.displayName,
        icon: formData.icon,
        circle: formData.circle,
        tier: formData.tier,
      },
      identity: {
        ...manifest.identity,
        role: formData.role,
        description: formData.description,
        communicationStyle: formData.communicationStyle || undefined,
        principles: formData.principles.length > 0 ? formData.principles : undefined,
      },
      model: {
        ...manifest.model,
        provider: formData.modelProvider,
        name: formData.modelName,
        temperature: formData.temperature,
      },
      tools: {
        allowed: formData.toolsAllowed.length > 0 ? formData.toolsAllowed : undefined,
        denied: formData.toolsDenied.length > 0 ? formData.toolsDenied : undefined,
      },
      skills: formData.skills.length > 0 ? formData.skills : undefined,
      resources: {
        memory: formData.memoryLimit || undefined,
        cpu: formData.cpuLimit || undefined,
      },
    };

    try {
      const res = await fetch(`/api/core/agents/${agentName}/manifest`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(updated),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || 'Save failed');
      setSuccess(true);
      setTimeout(() => setSuccess(false), 3000);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setSaving(false);
    }
  };

  const updateField = (field: string, value: any) => {
    setFormData(prev => ({ ...prev, [field]: value }));
  };

  const updateListField = (field: string, value: string) => {
    const items = value.split(',').map(s => s.trim()).filter(Boolean);
    setFormData(prev => ({ ...prev, [field]: items }));
  };

  if (loading) {
    return <div className="flex items-center justify-center h-full"><span className="text-sm text-sera-text-muted">Loading…</span></div>;
  }

  return (
    <div className="p-8 max-w-4xl mx-auto">
      {/* Breadcrumb */}
      <Link href={`/agents/${agentName}`} className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4">
        <ArrowLeft size={14} /> Back to {agentName}
      </Link>

      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <h1 className="sera-page-title">Edit Agent: {formData.displayName || agentName}</h1>
        <div className="flex items-center gap-2">
          {/* Mode Toggle */}
          <div className="flex items-center gap-0.5 bg-sera-surface border border-sera-border rounded-lg p-0.5">
            <button
              onClick={() => setMode('form')}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${mode === 'form' ? 'bg-sera-accent text-sera-bg' : 'text-sera-text-muted hover:text-sera-text'}`}
            >
              <FormInput size={12} /> Form
            </button>
            <button
              onClick={() => setMode('yaml')}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${mode === 'yaml' ? 'bg-sera-accent text-sera-bg' : 'text-sera-text-muted hover:text-sera-text'}`}
            >
              <Code size={12} /> YAML
            </button>
          </div>
          <button onClick={handleFormSave} disabled={saving} className="sera-btn-primary">
            {saving ? <RefreshCw size={14} className="animate-spin" /> : <Save size={14} />}
            Save
          </button>
        </div>
      </div>

      {/* Status Messages */}
      {error && (
        <div className="sera-card-static p-3 mb-4 border-sera-error/30 bg-sera-error/5">
          <p className="text-sm text-sera-error">{error}</p>
        </div>
      )}
      {success && (
        <div className="sera-card-static p-3 mb-4 border-sera-success/30 bg-sera-success/5">
          <p className="text-sm text-sera-success">Manifest saved and agents reloaded successfully!</p>
        </div>
      )}

      {/* Form Mode */}
      {mode === 'form' && (
        <div className="space-y-6">
          {/* Metadata */}
          <fieldset className="sera-card-static p-5">
            <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">Metadata</legend>
            <div className="grid grid-cols-2 gap-4 mt-2">
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Display Name</label>
                <input className="sera-input" value={formData.displayName} onChange={e => updateField('displayName', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Icon (emoji)</label>
                <input className="sera-input" value={formData.icon} onChange={e => updateField('icon', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Circle</label>
                <input className="sera-input" value={formData.circle} onChange={e => updateField('circle', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Security Tier</label>
                <select className="sera-input" value={formData.tier} onChange={e => updateField('tier', parseInt(e.target.value))}>
                  <option value={1}>Tier 1 — Restricted</option>
                  <option value={2}>Tier 2 — Standard</option>
                  <option value={3}>Tier 3 — Privileged</option>
                </select>
              </div>
            </div>
          </fieldset>

          {/* Identity */}
          <fieldset className="sera-card-static p-5">
            <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">Identity</legend>
            <div className="space-y-4 mt-2">
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Role</label>
                <input className="sera-input" value={formData.role} onChange={e => updateField('role', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Description</label>
                <textarea className="sera-input min-h-[80px]" value={formData.description} onChange={e => updateField('description', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Communication Style</label>
                <textarea className="sera-input min-h-[60px]" value={formData.communicationStyle} onChange={e => updateField('communicationStyle', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Principles (one per line)</label>
                <textarea
                  className="sera-input min-h-[80px]"
                  value={formData.principles.join('\n')}
                  onChange={e => updateField('principles', e.target.value.split('\n').filter(Boolean))}
                />
              </div>
            </div>
          </fieldset>

          {/* Model */}
          <fieldset className="sera-card-static p-5">
            <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">Model</legend>
            <div className="grid grid-cols-3 gap-4 mt-2">
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Provider</label>
                <input className="sera-input" value={formData.modelProvider} onChange={e => updateField('modelProvider', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Model Name</label>
                <input className="sera-input" value={formData.modelName} onChange={e => updateField('modelName', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Temperature</label>
                <input className="sera-input" type="number" step="0.1" min="0" max="2" value={formData.temperature} onChange={e => updateField('temperature', parseFloat(e.target.value))} />
              </div>
            </div>
          </fieldset>

          {/* Tools & Skills */}
          <fieldset className="sera-card-static p-5">
            <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">Tools & Skills</legend>
            <div className="space-y-4 mt-2">
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Allowed Tools (comma-separated)</label>
                <input className="sera-input" value={formData.toolsAllowed.join(', ')} onChange={e => updateListField('toolsAllowed', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Denied Tools (comma-separated)</label>
                <input className="sera-input" value={formData.toolsDenied.join(', ')} onChange={e => updateListField('toolsDenied', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Skills (comma-separated)</label>
                <input className="sera-input" value={formData.skills.join(', ')} onChange={e => updateListField('skills', e.target.value)} />
              </div>
            </div>
          </fieldset>

          {/* Resources */}
          <fieldset className="sera-card-static p-5">
            <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">Resources</legend>
            <div className="grid grid-cols-2 gap-4 mt-2">
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">Memory Limit</label>
                <input className="sera-input" placeholder="e.g. 512Mi" value={formData.memoryLimit} onChange={e => updateField('memoryLimit', e.target.value)} />
              </div>
              <div>
                <label className="text-xs text-sera-text-muted mb-1 block">CPU Limit</label>
                <input className="sera-input" placeholder="e.g. 0.5" value={formData.cpuLimit} onChange={e => updateField('cpuLimit', e.target.value)} />
              </div>
            </div>
          </fieldset>
        </div>
      )}

      {/* YAML Mode */}
      {mode === 'yaml' && (
        <div className="sera-card-static p-4">
          <textarea
            className="w-full bg-sera-bg border border-sera-border rounded-lg p-4 text-sm text-sera-text font-mono resize-y min-h-[500px] focus:outline-none focus:border-sera-border-active transition-colors"
            value={rawYaml}
            onChange={e => setRawYaml(e.target.value)}
            spellCheck={false}
          />
          <p className="text-[11px] text-sera-text-dim mt-2">
            ⚠️ Raw YAML editing is for advanced users. The form editor is recommended for most changes.
          </p>
        </div>
      )}
    </div>
  );
}
