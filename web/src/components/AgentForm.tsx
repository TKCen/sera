import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router';
import * as yaml from 'js-yaml';
import { ChevronDown, ChevronRight, Lock, AlertCircle, CheckCircle } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { useCircles } from '@/hooks/useCircles';
import { useSkills } from '@/hooks/useSkills';
import { useTools } from '@/hooks/useTools';
import { useTemplates } from '@/hooks/useTemplates';
import { useCreateAgent, useUpdateAgentManifest } from '@/hooks/useAgents';
import { validateAgentManifest } from '@/lib/api/agents';
import type { AgentManifest } from '@/lib/api/types';

interface AgentFormProps {
  initial?: AgentManifest;
  isEdit?: boolean;
}

const TIERS = [
  { value: '1', label: 'Tier 1', description: 'Unrestricted network + filesystem' },
  { value: '2', label: 'Tier 2', description: 'Restricted network, read-only filesystem' },
  { value: '3', label: 'Tier 3', description: 'No network, no filesystem' },
];

function defaultManifest(): AgentManifest {
  return {
    apiVersion: 'sera.dev/v1',
    kind: 'Agent',
    metadata: { name: '' },
    spec: {
      identity: { role: '', principles: [] },
      model: { provider: '', name: '', temperature: 0.7 },
      sandboxBoundary: 'tier-2',
      lifecycle: { mode: 'persistent' },
      skills: [],
      tools: { allowed: [], denied: [] },
      resources: {
        cpu: '0.5',
        memory: '512m',
        maxLlmTokensPerHour: 100000,
        maxLlmTokensPerDay: 1000000,
      },
    },
  };
}

function manifestToYaml(m: AgentManifest): string {
  try {
    return yaml.dump(m, { indent: 2, lineWidth: 120 });
  } catch {
    return '';
  }
}

function yamlToManifest(src: string): AgentManifest | null {
  try {
    return yaml.load(src) as AgentManifest;
  } catch {
    return null;
  }
}

export function AgentForm({ initial, isEdit = false }: AgentFormProps) {
  const navigate = useNavigate();
  const { data: circles = [] } = useCircles();
  const { data: skills = [] } = useSkills();
  const { data: tools = [] } = useTools();
  const { data: templates = [] } = useTemplates();

  const createAgent = useCreateAgent();
  const updateManifest = useUpdateAgentManifest();

  const [manifest, setManifest] = useState<AgentManifest>(initial ?? defaultManifest());
  const [selectedTemplate, setSelectedTemplate] = useState('');
  const [yamlSrc, setYamlSrc] = useState(() => manifestToYaml(initial ?? defaultManifest()));
  const [yamlOpen, setYamlOpen] = useState(false);
  const [yamlError, setYamlError] = useState('');
  const [validating, setValidating] = useState(false);
  const [validResult, setValidResult] = useState<{ valid: boolean; errors?: string[] } | null>(
    null
  );

  const syncingFromYaml = useRef(false);
  const syncingToYaml = useRef(false);

  const lockedFields = templates.find((t) => t.name === selectedTemplate)?.lockedFields ?? [];

  const isLocked = useCallback((field: string) => lockedFields.includes(field), [lockedFields]);

  function updateField(path: string[], value: unknown) {
    setManifest((prev) => {
      const next = structuredClone(prev);
      let cur: Record<string, unknown> = next as unknown as Record<string, unknown>;
      for (let i = 0; i < path.length - 1; i++) {
        const key = path[i];
        if (cur[key] == null || typeof cur[key] !== 'object') {
          cur[key] = {};
        }
        cur = cur[key] as Record<string, unknown>;
      }
      cur[path[path.length - 1]] = value;
      return next;
    });
  }

  useEffect(() => {
    if (syncingFromYaml.current) return;
    syncingToYaml.current = true;
    setYamlSrc(manifestToYaml(manifest));
    syncingToYaml.current = false;
  }, [manifest]);

  function handleYamlChange(src: string) {
    setYamlSrc(src);
    const parsed = yamlToManifest(src);
    if (parsed) {
      setYamlError('');
      syncingFromYaml.current = true;
      setManifest(parsed);
      syncingFromYaml.current = false;
    } else {
      setYamlError('Invalid YAML');
    }
  }

  async function handleValidate() {
    setValidating(true);
    setValidResult(null);
    try {
      const result = await validateAgentManifest(manifest);
      setValidResult(result);
    } catch {
      setValidResult({ valid: false, errors: ['Validation request failed'] });
    } finally {
      setValidating(false);
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!manifest.metadata.name) {
      toast.error('Agent name is required');
      return;
    }
    try {
      if (isEdit) {
        await updateManifest.mutateAsync({ name: manifest.metadata.name, manifest });
        toast.success('Agent updated');
      } else {
        await createAgent.mutateAsync(manifest);
        toast.success('Agent created');
      }
      void navigate(`/agents/${manifest.metadata.name}`);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to save agent');
    }
  }

  const isPending = createAgent.isPending || updateManifest.isPending;

  function FieldLabel({ label, field }: { label: string; field: string }) {
    return (
      <label className="block text-xs font-medium text-sera-text-muted mb-1">
        {label}
        {isLocked(field) && (
          <span className="ml-1.5 inline-flex items-center gap-0.5 text-sera-warning text-[10px]">
            <Lock size={9} /> locked
          </span>
        )}
      </label>
    );
  }

  function LockedInput({ field, value }: { field: string; value: string }) {
    if (isLocked(field)) {
      return (
        <div className="sera-input opacity-60 cursor-not-allowed bg-sera-surface-hover flex items-center gap-2">
          <Lock size={12} className="text-sera-warning flex-shrink-0" />
          <span className="text-sera-text-muted">{value || '—'}</span>
        </div>
      );
    }
    return null;
  }

  return (
    <form
      onSubmit={(e) => {
        void handleSubmit(e);
      }}
      className="space-y-6 pb-8"
    >
      {/* Template selector */}
      {!isEdit && templates.length > 0 && (
        <section className="sera-card-static p-4">
          <h3 className="text-sm font-semibold text-sera-text mb-3">Template</h3>
          <div>
            <FieldLabel label="Base template" field="templateRef" />
            <select
              value={selectedTemplate}
              onChange={(e) => {
                setSelectedTemplate(e.target.value);
                if (e.target.value) {
                  updateField(['metadata', 'templateRef'], e.target.value);
                }
              }}
              className="sera-input"
            >
              <option value="">— No template —</option>
              {templates.map((t) => (
                <option key={t.name} value={t.name}>
                  {t.displayName ?? t.name}
                  {t.description ? ` — ${t.description}` : ''}
                </option>
              ))}
            </select>
          </div>
        </section>
      )}

      {/* Identity */}
      <section className="sera-card-static p-4 space-y-4">
        <h3 className="text-sm font-semibold text-sera-text">Identity</h3>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <FieldLabel label="Name *" field="name" />
            {isLocked('name') ? (
              <LockedInput field="name" value={manifest.metadata.name} />
            ) : (
              <Input
                value={manifest.metadata.name}
                onChange={(e) => updateField(['metadata', 'name'], e.target.value)}
                placeholder="my-agent"
                disabled={isEdit}
                required
              />
            )}
          </div>
          <div>
            <FieldLabel label="Display name" field="displayName" />
            {isLocked('displayName') ? (
              <LockedInput field="displayName" value={manifest.metadata.displayName ?? ''} />
            ) : (
              <Input
                value={manifest.metadata.displayName ?? ''}
                onChange={(e) => updateField(['metadata', 'displayName'], e.target.value)}
                placeholder="My Agent"
              />
            )}
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <FieldLabel label="Circle" field="circle" />
            {isLocked('circle') ? (
              <LockedInput field="circle" value={manifest.metadata.circle ?? ''} />
            ) : (
              <select
                value={manifest.metadata.circle ?? ''}
                onChange={(e) => updateField(['metadata', 'circle'], e.target.value || undefined)}
                className="sera-input"
              >
                <option value="">— No circle —</option>
                {circles.map((c) => (
                  <option key={c.name} value={c.name}>
                    {c.displayName ?? c.name}
                  </option>
                ))}
              </select>
            )}
          </div>
          <div>
            <FieldLabel label="Lifecycle" field="lifecycle" />
            {isLocked('lifecycle') ? (
              <LockedInput
                field="lifecycle"
                value={manifest.spec?.lifecycle?.mode ?? 'persistent'}
              />
            ) : (
              <select
                value={manifest.spec?.lifecycle?.mode ?? 'persistent'}
                onChange={(e) => updateField(['spec', 'lifecycle', 'mode'], e.target.value)}
                className="sera-input"
              >
                <option value="persistent">Persistent</option>
                <option value="ephemeral">Ephemeral</option>
              </select>
            )}
          </div>
        </div>

        <div>
          <FieldLabel label="Role" field="role" />
          {isLocked('role') ? (
            <LockedInput field="role" value={manifest.spec?.identity?.role ?? ''} />
          ) : (
            <textarea
              value={manifest.spec?.identity?.role ?? ''}
              onChange={(e) => updateField(['spec', 'identity', 'role'], e.target.value)}
              placeholder="Describe this agent's role and responsibilities…"
              rows={3}
              className="sera-input resize-y"
            />
          )}
        </div>
      </section>

      {/* Sandbox tier */}
      <section className="sera-card-static p-4">
        <h3 className="text-sm font-semibold text-sera-text mb-3">Sandbox Tier</h3>
        <FieldLabel label="Boundary" field="sandboxBoundary" />
        {isLocked('sandboxBoundary') ? (
          <LockedInput field="sandboxBoundary" value={manifest.spec?.sandboxBoundary ?? 'tier-2'} />
        ) : (
          <div className="grid grid-cols-3 gap-2">
            {TIERS.map((tier) => {
              const val = `tier-${tier.value}`;
              const active = (manifest.spec?.sandboxBoundary ?? 'tier-2') === val;
              return (
                <button
                  key={tier.value}
                  type="button"
                  onClick={() => updateField(['spec', 'sandboxBoundary'], val)}
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
        )}
      </section>

      {/* Model */}
      <section className="sera-card-static p-4 space-y-4">
        <h3 className="text-sm font-semibold text-sera-text">Model</h3>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <FieldLabel label="Provider" field="model.provider" />
            {isLocked('model.provider') ? (
              <LockedInput field="model.provider" value={manifest.spec?.model?.provider ?? ''} />
            ) : (
              <Input
                value={manifest.spec?.model?.provider ?? ''}
                onChange={(e) => updateField(['spec', 'model', 'provider'], e.target.value)}
                placeholder="openai"
              />
            )}
          </div>
          <div>
            <FieldLabel label="Model name" field="model.name" />
            {isLocked('model.name') ? (
              <LockedInput field="model.name" value={manifest.spec?.model?.name ?? ''} />
            ) : (
              <Input
                value={manifest.spec?.model?.name ?? ''}
                onChange={(e) => updateField(['spec', 'model', 'name'], e.target.value)}
                placeholder="gpt-4o"
              />
            )}
          </div>
        </div>
        <div>
          <FieldLabel
            label={`Temperature: ${manifest.spec?.model?.temperature ?? 0.7}`}
            field="model.temperature"
          />
          {isLocked('model.temperature') ? (
            <LockedInput
              field="model.temperature"
              value={String(manifest.spec?.model?.temperature ?? 0.7)}
            />
          ) : (
            <input
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={manifest.spec?.model?.temperature ?? 0.7}
              onChange={(e) =>
                updateField(['spec', 'model', 'temperature'], parseFloat(e.target.value))
              }
              className="w-full accent-sera-accent"
            />
          )}
        </div>
      </section>

      {/* Skills */}
      {skills.length > 0 && (
        <section className="sera-card-static p-4">
          <h3 className="text-sm font-semibold text-sera-text mb-3">Skills</h3>
          <FieldLabel label="Assigned skills" field="skills" />
          {isLocked('skills') ? (
            <LockedInput field="skills" value={(manifest.spec?.skills ?? []).join(', ')} />
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {skills.map((skill) => {
                const active = (manifest.spec?.skills ?? []).includes(skill.id);
                return (
                  <button
                    key={skill.id}
                    type="button"
                    onClick={() => {
                      const current = manifest.spec?.skills ?? [];
                      updateField(
                        ['spec', 'skills'],
                        active ? current.filter((s) => s !== skill.id) : [...current, skill.id]
                      );
                    }}
                    className={cn(
                      'px-2 py-1 rounded-md text-xs border transition-colors',
                      active
                        ? 'border-sera-accent bg-sera-accent-soft text-sera-accent'
                        : 'border-sera-border text-sera-text-muted hover:border-sera-border-active'
                    )}
                  >
                    {skill.name ?? skill.id}
                  </button>
                );
              })}
            </div>
          )}
        </section>
      )}

      {/* Tools */}
      {tools.length > 0 && (
        <section className="sera-card-static p-4">
          <h3 className="text-sm font-semibold text-sera-text mb-3">Tools</h3>
          <FieldLabel label="Allowed tools" field="tools" />
          {isLocked('tools') ? (
            <LockedInput field="tools" value={(manifest.spec?.tools?.allowed ?? []).join(', ')} />
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {tools.map((tool) => {
                const allowed = (manifest.spec?.tools?.allowed ?? []).includes(tool.id);
                return (
                  <button
                    key={tool.id}
                    type="button"
                    onClick={() => {
                      const current = manifest.spec?.tools?.allowed ?? [];
                      updateField(
                        ['spec', 'tools', 'allowed'],
                        allowed ? current.filter((t) => t !== tool.id) : [...current, tool.id]
                      );
                    }}
                    className={cn(
                      'px-2 py-1 rounded-md text-xs border transition-colors',
                      allowed
                        ? 'border-sera-accent bg-sera-accent-soft text-sera-accent'
                        : 'border-sera-border text-sera-text-muted hover:border-sera-border-active'
                    )}
                  >
                    {tool.name ?? tool.id}
                  </button>
                );
              })}
            </div>
          )}
        </section>
      )}

      {/* Resources */}
      <section className="sera-card-static p-4 space-y-4">
        <h3 className="text-sm font-semibold text-sera-text">Resources</h3>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <FieldLabel label="Tokens / hour" field="resources.maxLlmTokensPerHour" />
            <Input
              type="number"
              value={manifest.spec?.resources?.maxLlmTokensPerHour ?? 100000}
              onChange={(e) =>
                updateField(['spec', 'resources', 'maxLlmTokensPerHour'], parseInt(e.target.value))
              }
              disabled={isLocked('resources.maxLlmTokensPerHour')}
            />
          </div>
          <div>
            <FieldLabel label="Tokens / day" field="resources.maxLlmTokensPerDay" />
            <Input
              type="number"
              value={manifest.spec?.resources?.maxLlmTokensPerDay ?? 1000000}
              onChange={(e) =>
                updateField(['spec', 'resources', 'maxLlmTokensPerDay'], parseInt(e.target.value))
              }
              disabled={isLocked('resources.maxLlmTokensPerDay')}
            />
          </div>
          <div>
            <FieldLabel label="CPU" field="resources.cpu" />
            <Input
              value={manifest.spec?.resources?.cpu ?? ''}
              onChange={(e) => updateField(['spec', 'resources', 'cpu'], e.target.value)}
              placeholder="0.5"
              disabled={isLocked('resources.cpu')}
            />
          </div>
          <div>
            <FieldLabel label="Memory" field="resources.memory" />
            <Input
              value={manifest.spec?.resources?.memory ?? ''}
              onChange={(e) => updateField(['spec', 'resources', 'memory'], e.target.value)}
              placeholder="512m"
              disabled={isLocked('resources.memory')}
            />
          </div>
        </div>
      </section>

      {/* Advanced: YAML editor */}
      <section className="sera-card-static">
        <button
          type="button"
          onClick={() => setYamlOpen((o) => !o)}
          className="flex items-center gap-2 w-full px-4 py-3 text-sm font-semibold text-sera-text hover:bg-sera-surface-hover transition-colors rounded-xl"
        >
          {yamlOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          Advanced — YAML editor
        </button>
        {yamlOpen && (
          <div className="px-4 pb-4">
            {yamlError && (
              <p className="text-xs text-sera-error mb-2 flex items-center gap-1">
                <AlertCircle size={11} /> {yamlError}
              </p>
            )}
            <textarea
              value={yamlSrc}
              onChange={(e) => handleYamlChange(e.target.value)}
              rows={20}
              className="sera-input font-mono text-xs resize-y"
              spellCheck={false}
            />
          </div>
        )}
      </section>

      {/* Validate + submit */}
      <div className="flex items-center gap-3 pt-2">
        <Button type="submit" disabled={isPending}>
          {isPending ? 'Saving…' : isEdit ? 'Save changes' : 'Create agent'}
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={validating}
          onClick={() => {
            void handleValidate();
          }}
        >
          {validating ? 'Validating…' : 'Validate manifest'}
        </Button>
        <Button type="button" variant="ghost" onClick={() => navigate(-1)}>
          Cancel
        </Button>

        {validResult && (
          <span
            className={cn(
              'flex items-center gap-1 text-xs ml-2',
              validResult.valid ? 'text-sera-success' : 'text-sera-error'
            )}
          >
            {validResult.valid ? (
              <>
                <CheckCircle size={13} /> Valid
              </>
            ) : (
              <>
                <AlertCircle size={13} /> {validResult.errors?.join(', ') ?? 'Invalid'}
              </>
            )}
          </span>
        )}
      </div>
    </form>
  );
}
