import { useState, useMemo, useEffect } from 'react';
import { useNavigate, useSearchParams } from 'react-router';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { useCircles } from '@/hooks/useCircles';
import { useTemplates } from '@/hooks/useTemplates';
import { useCreateAgent, useUpdateAgent } from '@/hooks/useAgents';
import { useProviders } from '@/hooks/useProviders';
import { useTools } from '@/hooks/useTools';
import { useSkills } from '@/hooks/useSkills';
import { MultiSelectPicker } from '@/components/MultiSelectPicker';
import type { PickerItem } from '@/components/MultiSelectPicker';

const TIERS = [
  {
    value: '1',
    label: 'Tier 1',
    description: 'No network, read-only filesystem (most restricted)',
  },
  {
    value: '2',
    label: 'Tier 2',
    description: 'Restricted network via egress proxy, read-only root',
  },
  { value: '3', label: 'Tier 3', description: 'Command-restricted, network via egress proxy' },
];

export interface AgentFormInitialValues {
  templateRef?: string;
  name?: string;
  displayName?: string;
  circle?: string;
  lifecycleMode?: 'persistent' | 'ephemeral';
  modelName?: string;
  modelProvider?: string;
  temperature?: number;
  sandboxBoundary?: string;
  tokensPerHour?: number;
  tokensPerDay?: number;
  canExec?: boolean;
  canSpawnSubagents?: boolean;
  toolsAllowed?: string[];
  toolsDenied?: string[];
  skills?: string[];
}

interface AgentFormProps {
  mode?: 'create' | 'edit';
  instanceId?: string;
  initialValues?: AgentFormInitialValues;
  onSuccess?: (id: string) => void;
  onCancel?: () => void;
}

export function AgentForm({
  mode = 'create',
  instanceId,
  initialValues,
  onSuccess,
  onCancel,
}: AgentFormProps) {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { data: circles = [] } = useCircles();
  const { data: templates = [] } = useTemplates();
  const { data: providersData } = useProviders();
  const { data: toolsData, isLoading: toolsLoading } = useTools();
  const { data: skillsData, isLoading: skillsLoading } = useSkills();
  const availableModels = providersData?.providers ?? [];
  const createAgent = useCreateAgent();
  const updateAgent = useUpdateAgent();

  const isEdit = mode === 'edit';

  // Pre-select template from query param or initialValues
  const [selectedTemplate, setSelectedTemplate] = useState(
    initialValues?.templateRef ?? searchParams.get('template') ?? ''
  );
  const [name, setName] = useState(initialValues?.name ?? '');
  const [displayName, setDisplayName] = useState(initialValues?.displayName ?? '');
  const [circle, setCircle] = useState(initialValues?.circle ?? '');
  const [lifecycleMode, setLifecycleMode] = useState<'persistent' | 'ephemeral'>(
    initialValues?.lifecycleMode ?? 'persistent'
  );
  const [modelName, setModelName] = useState(initialValues?.modelName ?? '');
  const [modelProvider, setModelProvider] = useState(initialValues?.modelProvider ?? '');
  const [temperature, setTemperature] = useState(initialValues?.temperature ?? 0.7);
  const [sandboxBoundary, setSandboxBoundary] = useState(
    initialValues?.sandboxBoundary ?? 'tier-2'
  );
  const [tokensPerHour, setTokensPerHour] = useState(initialValues?.tokensPerHour ?? 100000);
  const [tokensPerDay, setTokensPerDay] = useState(initialValues?.tokensPerDay ?? 1000000);
  const [canExec, setCanExec] = useState(initialValues?.canExec ?? false);
  const [canSpawnSubagents, setCanSpawnSubagents] = useState(
    initialValues?.canSpawnSubagents ?? false
  );
  const [toolsAllowed, setToolsAllowed] = useState<string[]>(initialValues?.toolsAllowed ?? []);
  const [toolsDenied, setToolsDenied] = useState<string[]>(initialValues?.toolsDenied ?? []);
  const [skills, setSkills] = useState<string[]>(initialValues?.skills ?? []);
  const [autoStart, setAutoStart] = useState(true);

  // Sync initialValues when they change (e.g. async load)
  useEffect(() => {
    if (!initialValues) return;
    if (initialValues.templateRef) setSelectedTemplate(initialValues.templateRef);
    if (initialValues.name) setName(initialValues.name);
    if (initialValues.displayName !== undefined) setDisplayName(initialValues.displayName);
    if (initialValues.circle !== undefined) setCircle(initialValues.circle);
    if (initialValues.lifecycleMode) setLifecycleMode(initialValues.lifecycleMode);
    if (initialValues.modelName) setModelName(initialValues.modelName);
    if (initialValues.modelProvider) setModelProvider(initialValues.modelProvider);
    if (initialValues.temperature !== undefined) setTemperature(initialValues.temperature);
    if (initialValues.sandboxBoundary) setSandboxBoundary(initialValues.sandboxBoundary);
    if (initialValues.tokensPerHour !== undefined) setTokensPerHour(initialValues.tokensPerHour);
    if (initialValues.tokensPerDay !== undefined) setTokensPerDay(initialValues.tokensPerDay);
    if (initialValues.canExec !== undefined) setCanExec(initialValues.canExec);
    if (initialValues.canSpawnSubagents !== undefined)
      setCanSpawnSubagents(initialValues.canSpawnSubagents);
    if (initialValues.toolsAllowed) setToolsAllowed(initialValues.toolsAllowed);
    if (initialValues.toolsDenied) setToolsDenied(initialValues.toolsDenied);
    if (initialValues.skills) setSkills(initialValues.skills);
  }, [initialValues]);

  // When template changes, pre-fill defaults from template spec
  const template = useMemo(
    () => templates.find((t) => t.name === selectedTemplate),
    [templates, selectedTemplate]
  );

  const currentTier = useMemo(() => {
    const m = sandboxBoundary.match(/\d/);
    return m ? parseInt(m[0]!, 10) : 2;
  }, [sandboxBoundary]);

  const toolPickerItems: PickerItem[] = useMemo(() => {
    const tools = toolsData ?? [];
    return [...tools]
      .sort((a, b) => {
        if (a.source === 'builtin' && b.source !== 'builtin') return -1;
        if (a.source !== 'builtin' && b.source === 'builtin') return 1;
        return (a.server ?? '').localeCompare(b.server ?? '');
      })
      .map((t) => {
        const needsHigherTier = t.minTier != null && t.minTier > currentTier;
        return {
          id: t.id,
          label: t.id,
          description: t.description,
          group: t.source === 'builtin' ? 'Builtin Tools' : `MCP: ${t.server ?? 'unknown'}`,
          badge: t.source,
          badgeVariant: (t.source === 'builtin' ? 'default' : 'accent') as 'default' | 'accent',
          ...(needsHigherTier ? { warning: `Requires Tier ${t.minTier}+` } : {}),
        };
      });
  }, [toolsData, currentTier]);

  const skillPickerItems: PickerItem[] = useMemo(
    () =>
      (skillsData ?? []).map((s) => ({
        id: s.id ?? s.name ?? '',
        label: s.name ?? s.id ?? '',
        description: s.description,
        group: s.category ?? 'General',
        ...(s.maxTokens != null ? { tokenCost: s.maxTokens } : {}),
      })),
    [skillsData]
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
    if (!isEdit && !selectedTemplate) {
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

    const overrides: Record<string, unknown> = {
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
      permissions: {
        canExec,
        canSpawnSubagents,
      },
      ...(toolsAllowed.length > 0 || toolsDenied.length > 0
        ? {
            tools: {
              ...(toolsAllowed.length > 0 ? { allowed: toolsAllowed } : {}),
              ...(toolsDenied.length > 0 ? { denied: toolsDenied } : {}),
            },
          }
        : {}),
      ...(skills.length > 0 ? { skills } : {}),
    };

    try {
      if (isEdit && instanceId) {
        await updateAgent.mutateAsync({
          id: instanceId,
          params: {
            name,
            displayName: displayName || undefined,
            circle: circle || undefined,
            lifecycleMode,
            overrides,
          },
        });
        toast.success(`Agent "${name}" updated`);
        if (onSuccess) {
          onSuccess(instanceId);
        } else {
          void navigate(`/agents/${instanceId}`);
        }
      } else {
        const newAgent = await createAgent.mutateAsync({
          templateRef: selectedTemplate,
          name,
          displayName: displayName || undefined,
          circle: circle || undefined,
          lifecycleMode,
          start: autoStart,
          overrides,
        });
        toast.success(`Agent "${name}" created`);
        if (onSuccess) {
          onSuccess(newAgent.id);
        } else {
          void navigate(`/agents/${newAgent.id}`);
        }
      }
    } catch (err) {
      toast.error(
        err instanceof Error ? err.message : `Failed to ${isEdit ? 'update' : 'create'} agent`
      );
    }
  }

  const isPending = isEdit ? updateAgent.isPending : createAgent.isPending;

  return (
    <form
      onSubmit={(e) => {
        void handleSubmit(e);
      }}
      className="space-y-6 pb-8"
    >
      {/* Template selector — hidden in edit mode */}
      {!isEdit && (
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
      )}

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
                  Settings &rarr; Providers
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

      {/* Permissions */}
      <section className="sera-card-static p-4 space-y-4">
        <h3 className="text-sm font-semibold text-sera-text">Permissions</h3>
        <div className="space-y-3">
          <label className="flex items-center gap-2 text-sm text-sera-text">
            <input
              type="checkbox"
              checked={canExec}
              onChange={(e) => setCanExec(e.target.checked)}
              className="accent-sera-accent"
            />
            Can execute commands
          </label>
          <label className="flex items-center gap-2 text-sm text-sera-text">
            <input
              type="checkbox"
              checked={canSpawnSubagents}
              onChange={(e) => setCanSpawnSubagents(e.target.checked)}
              className="accent-sera-accent"
            />
            Can spawn subagents
          </label>
        </div>
      </section>

      {/* Tools & Skills */}
      <section className="sera-card-static p-4 space-y-4">
        <h3 className="text-sm font-semibold text-sera-text">Tools &amp; Skills</h3>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1.5">
              Tools Allowed
            </label>
            <MultiSelectPicker
              items={toolPickerItems}
              selected={toolsAllowed}
              onChange={setToolsAllowed}
              placeholder="Search tools…"
              loading={toolsLoading}
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1.5">
              Tools Denied
            </label>
            <MultiSelectPicker
              items={toolPickerItems}
              selected={toolsDenied}
              onChange={setToolsDenied}
              placeholder="Search tools…"
              loading={toolsLoading}
            />
          </div>
        </div>
        <div>
          <label className="block text-xs font-medium text-sera-text-muted mb-1.5">Skills</label>
          <MultiSelectPicker
            items={skillPickerItems}
            selected={skills}
            onChange={setSkills}
            placeholder="Search skills…"
            loading={skillsLoading}
          />
        </div>
      </section>

      {/* Options — only show in create mode */}
      {!isEdit && (
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
      )}

      {/* Submit */}
      <div className="flex items-center gap-3 pt-2">
        <Button type="submit" disabled={isPending}>
          {isPending
            ? isEdit
              ? 'Saving…'
              : 'Creating…'
            : isEdit
              ? 'Save Changes'
              : 'Create Agent'}
        </Button>
        <Button type="button" variant="ghost" onClick={() => (onCancel ? onCancel() : navigate(-1))}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
