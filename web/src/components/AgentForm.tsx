import { useState, useMemo } from 'react';
import { useNavigate, useSearchParams } from 'react-router';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { useCircles } from '@/hooks/useCircles';
import { useTemplates } from '@/hooks/useTemplates';
import { useCreateAgent } from '@/hooks/useAgents';
import { useProviders } from '@/hooks/useProviders';

const TIERS = [
  { value: '1', label: 'Tier 1', description: 'Unrestricted network + filesystem' },
  { value: '2', label: 'Tier 2', description: 'Restricted network, read-only filesystem' },
  { value: '3', label: 'Tier 3', description: 'No network, no filesystem' },
];

export function AgentForm() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { data: circles = [] } = useCircles();
  const { data: templates = [] } = useTemplates();
  const { data: providersData } = useProviders();
  const availableModels = providersData?.providers ?? [];
  const createAgent = useCreateAgent();

  // Pre-select template from query param
  const [selectedTemplate, setSelectedTemplate] = useState(searchParams.get('template') ?? '');
  const [name, setName] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [circle, setCircle] = useState('');
  const [lifecycleMode, setLifecycleMode] = useState<'persistent' | 'ephemeral'>('persistent');
  const [modelName, setModelName] = useState('');
  const [modelProvider, setModelProvider] = useState('');
  const [temperature, setTemperature] = useState(0.7);
  const [sandboxBoundary, setSandboxBoundary] = useState('tier-2');
  const [tokensPerHour, setTokensPerHour] = useState(100000);
  const [tokensPerDay, setTokensPerDay] = useState(1000000);
  const [autoStart, setAutoStart] = useState(true);

  // When template changes, pre-fill defaults from template spec
  const template = useMemo(
    () => templates.find((t) => t.name === selectedTemplate),
    [templates, selectedTemplate]
  );

  function handleTemplateChange(templateName: string) {
    setSelectedTemplate(templateName);
    const t = templates.find((tpl) => tpl.name === templateName);
    if (t?.spec) {
      const spec = t.spec as Record<string, unknown>;
      const lifecycle = spec.lifecycle as Record<string, string> | undefined;
      const resources = spec.resources as Record<string, unknown> | undefined;
      if (lifecycle?.mode === 'ephemeral') setLifecycleMode('ephemeral');
      if (typeof spec.sandboxBoundary === 'string') setSandboxBoundary(spec.sandboxBoundary);
      if (resources?.maxLlmTokensPerHour) setTokensPerHour(resources.maxLlmTokensPerHour as number);
      if (resources?.maxLlmTokensPerDay) setTokensPerDay(resources.maxLlmTokensPerDay as number);
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!selectedTemplate) {
      toast.error('Please select a template');
      return;
    }
    if (!name) {
      toast.error('Agent name is required');
      return;
    }
    if (!modelName) {
      toast.error('Please select a model');
      return;
    }
    try {
      await createAgent.mutateAsync({
        templateRef: selectedTemplate,
        name,
        displayName: displayName || undefined,
        circle: circle || undefined,
        lifecycleMode,
        start: autoStart,
        overrides: {
          model: {
            provider: modelProvider,
            name: modelName,
            temperature,
          },
          sandboxBoundary,
          resources: {
            maxLlmTokensPerHour: tokensPerHour,
            maxLlmTokensPerDay: tokensPerDay,
          },
        },
      });
      toast.success(`Agent "${name}" created`);
      void navigate('/agents');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to create agent');
    }
  }

  return (
    <form
      onSubmit={(e) => {
        void handleSubmit(e);
      }}
      className="space-y-6 pb-8"
    >
      {/* Template selector */}
      <section className="sera-card-static p-4">
        <h3 className="text-sm font-semibold text-sera-text mb-3">Template *</h3>
        {templates.length === 0 ? (
          <p className="text-xs text-sera-text-muted">
            No templates available. Templates are loaded from <code>templates/builtin/</code> on
            startup.
          </p>
        ) : (
          <select
            value={selectedTemplate}
            onChange={(e) => handleTemplateChange(e.target.value)}
            className="sera-input"
            required
          >
            <option value="">— Select a template —</option>
            {templates.map((t) => (
              <option key={t.name} value={t.name}>
                {t.displayName ?? t.name}
                {t.description ? ` — ${t.description}` : ''}
              </option>
            ))}
          </select>
        )}
        {template?.description && (
          <p className="text-xs text-sera-text-dim mt-2">{template.description}</p>
        )}
      </section>

      {/* Identity */}
      <section className="sera-card-static p-4 space-y-4">
        <h3 className="text-sm font-semibold text-sera-text">Identity</h3>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1">Name *</label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="my-agent"
              required
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1">
              Display name
            </label>
            <Input
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="My Agent"
            />
          </div>
        </div>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1">Circle</label>
            <select
              value={circle}
              onChange={(e) => setCircle(e.target.value)}
              className="sera-input"
            >
              <option value="">— No circle —</option>
              {circles.map((c) => (
                <option key={c.name} value={c.name}>
                  {c.displayName ?? c.name}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1">Lifecycle</label>
            <select
              value={lifecycleMode}
              onChange={(e) => setLifecycleMode(e.target.value as 'persistent' | 'ephemeral')}
              className="sera-input"
            >
              <option value="persistent">Persistent</option>
              <option value="ephemeral">Ephemeral</option>
            </select>
          </div>
        </div>
      </section>

      {/* Sandbox tier */}
      <section className="sera-card-static p-4">
        <h3 className="text-sm font-semibold text-sera-text mb-3">Sandbox Tier</h3>
        <div className="grid grid-cols-3 gap-2">
          {TIERS.map((tier) => {
            const val = `tier-${tier.value}`;
            const active = sandboxBoundary === val;
            return (
              <button
                key={tier.value}
                type="button"
                onClick={() => setSandboxBoundary(val)}
                className={cn(
                  'p-3 rounded-lg border text-left transition-colors',
                  active
                    ? 'border-sera-accent bg-sera-accent-soft text-sera-accent'
                    : 'border-sera-border bg-sera-surface text-sera-text-muted hover:border-sera-border-active'
                )}
              >
                <div className="text-xs font-semibold">{tier.label}</div>
                <div className="text-[11px] mt-0.5">{tier.description}</div>
              </button>
            );
          })}
        </div>
      </section>

      {/* Model */}
      <section className="sera-card-static p-4 space-y-4">
        <h3 className="text-sm font-semibold text-sera-text">Model *</h3>
        <div>
          {availableModels.length > 0 ? (
            <select
              value={modelName}
              onChange={(e) => {
                setModelName(e.target.value);
                const sel = availableModels.find((m) => m.modelName === e.target.value);
                if (sel?.provider) setModelProvider(sel.provider);
              }}
              className="sera-input"
              required
            >
              <option value="">— Select a model —</option>
              {availableModels.map((m) => (
                <option key={m.modelName} value={m.modelName}>
                  {m.description ?? m.modelName}
                </option>
              ))}
            </select>
          ) : (
            <div className="space-y-2">
              <Input
                value={modelName}
                onChange={(e) => setModelName(e.target.value)}
                placeholder="model-name"
                required
              />
              <p className="text-[11px] text-sera-text-muted">
                No models available. Configure an LLM provider in{' '}
                <a href="/settings" className="text-sera-accent hover:underline">
                  Settings → Providers
                </a>
                .
              </p>
            </div>
          )}
        </div>
        <div>
          <label className="block text-xs font-medium text-sera-text-muted mb-1">
            Temperature: {temperature}
          </label>
          <input
            type="range"
            min="0"
            max="1"
            step="0.05"
            value={temperature}
            onChange={(e) => setTemperature(parseFloat(e.target.value))}
            className="w-full accent-sera-accent"
          />
        </div>
      </section>

      {/* Resources */}
      <section className="sera-card-static p-4 space-y-4">
        <h3 className="text-sm font-semibold text-sera-text">Resources</h3>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1">
              Tokens / hour
            </label>
            <Input
              type="number"
              value={tokensPerHour}
              onChange={(e) => setTokensPerHour(parseInt(e.target.value))}
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1">
              Tokens / day
            </label>
            <Input
              type="number"
              value={tokensPerDay}
              onChange={(e) => setTokensPerDay(parseInt(e.target.value))}
            />
          </div>
        </div>
      </section>

      {/* Options */}
      <section className="sera-card-static p-4">
        <label className="flex items-center gap-2 text-sm text-sera-text">
          <input
            type="checkbox"
            checked={autoStart}
            onChange={(e) => setAutoStart(e.target.checked)}
            className="accent-sera-accent"
          />
          Start agent after creation
        </label>
      </section>

      {/* Submit */}
      <div className="flex items-center gap-3 pt-2">
        <Button type="submit" disabled={createAgent.isPending}>
          {createAgent.isPending ? 'Creating…' : 'Create Agent'}
        </Button>
        <Button type="button" variant="ghost" onClick={() => navigate(-1)}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
