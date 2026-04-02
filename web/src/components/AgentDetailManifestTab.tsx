import { useState } from 'react';
import { useAgent, useAgentTools } from '@/hooks/useAgents';
import { cn } from '@/lib/utils';
import { TabLoading } from '@/components/AgentDetailTabLoading';
import { AlertCircle, CheckCircle2 } from 'lucide-react';

export function AgentDetailManifestTab({ id }: { id: string }) {
  const { data: instance, isLoading } = useAgent(id);
  const { data: toolsData } = useAgentTools(id);
  const [showRaw, setShowRaw] = useState(false);

  if (isLoading) return <TabLoading />;
  if (!instance) return <div className="p-6 text-sm text-sera-text-muted">Instance not found.</div>;

  const inst = instance as unknown as Record<string, unknown>;
  const overrides = (inst.overrides ?? {}) as Record<string, unknown>;
  const modelOv = overrides.model as Record<string, unknown> | undefined;
  const resolvedConfig = (inst.resolved_config ?? {}) as Record<string, unknown>;
  const specResources = (resolvedConfig.resources ??
    (resolvedConfig.spec as Record<string, unknown> | undefined)?.resources ??
    {}) as Record<string, unknown>;
  const resourcesOv = (overrides.resources ?? specResources) as Record<string, unknown> | undefined;
  const resolvedCaps = (inst.resolved_capabilities ?? {}) as Record<string, unknown>;
  const permissions = overrides.permissions as Record<string, unknown> | undefined;
  const tools = overrides.tools as Record<string, unknown> | undefined;
  const skills = (overrides.skills as string[] | undefined) ?? [];

  return (
    <div className="p-6 space-y-4 max-w-3xl">
      {/* Identity */}
      <section className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Identity
        </h3>
        <div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
          <Field label="Name" value={inst.name as string} />
          <Field label="Display Name" value={(inst.display_name as string) || '—'} />
          <Field
            label="Template"
            value={(inst.template_ref as string) || (inst.template_name as string)}
          />
          <Field label="Circle" value={(inst.circle as string) || '—'} />
          <Field label="Lifecycle" value={(inst.lifecycle_mode as string) || 'persistent'} />
          <Field label="Workspace" value={(inst.workspace_path as string) || '—'} mono />
        </div>
      </section>

      {/* Model & Sandbox */}
      <section className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Model &amp; Sandbox
        </h3>
        <div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
          <Field label="Model" value={(modelOv?.name as string) || 'default'} mono />
          <Field label="Provider" value={(modelOv?.provider as string) || '—'} />
          <Field label="Temperature" value={String(modelOv?.temperature ?? '0.7')} />
          <Field
            label="Sandbox Boundary"
            value={
              (overrides.sandboxBoundary as string) || (inst.sandbox_boundary as string) || '—'
            }
          />
        </div>
      </section>

      {/* Resources */}
      <section className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Resources
        </h3>
        <div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
          <Field
            label="Tokens / Hour"
            value={
              resourcesOv?.maxLlmTokensPerHour
                ? (resourcesOv.maxLlmTokensPerHour as number).toLocaleString()
                : '—'
            }
          />
          <Field
            label="Tokens / Day"
            value={
              resourcesOv?.maxLlmTokensPerDay
                ? (resourcesOv.maxLlmTokensPerDay as number).toLocaleString()
                : '—'
            }
          />
        </div>
      </section>

      {/* Permissions & Tools */}
      {(permissions || tools || skills.length > 0) && (
        <section className="sera-card-static p-4">
          <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
            Permissions &amp; Tools
          </h3>
          <div className="space-y-2 text-xs">
            {permissions?.canExec !== undefined && (
              <Field
                label="Can Execute"
                value={String(permissions.canExec) === 'true' ? 'Yes' : 'No'}
              />
            )}
            {permissions?.canSpawnSubagents !== undefined && (
              <Field
                label="Can Spawn Subagents"
                value={String(permissions.canSpawnSubagents) === 'true' ? 'Yes' : 'No'}
              />
            )}
            {Array.isArray(tools?.allowed) && (tools.allowed as string[]).length > 0 && (
              <div>
                <span className="text-sera-text-muted">Tools Allowed: </span>
                <div className="flex flex-wrap gap-2 mt-1">
                  {(tools.allowed as string[]).map((toolId) => {
                    const isAvailable = toolsData?.available.some((t) => t.id === toolId);
                    const isUnavailable = toolsData?.unavailable.includes(toolId);
                    const statusIcon = isAvailable ? (
                      <CheckCircle2 size={10} className="text-sera-success" />
                    ) : isUnavailable ? (
                      <AlertCircle size={10} className="text-sera-warning" />
                    ) : null;

                    return (
                      <div
                        key={toolId}
                        className={cn(
                          'inline-flex items-center gap-1.5 px-2 py-0.5 rounded border font-mono text-[11px]',
                          isAvailable
                            ? 'bg-sera-success/5 border-sera-success/20 text-sera-success'
                            : isUnavailable
                              ? 'bg-sera-warning/5 border-sera-warning/20 text-sera-warning'
                              : 'bg-sera-surface border-sera-border text-sera-text'
                        )}
                      >
                        {statusIcon}
                        {toolId}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
            {Array.isArray(tools?.denied) && (tools.denied as string[]).length > 0 && (
              <div>
                <span className="text-sera-text-muted">Tools Denied: </span>
                <span className="text-sera-text font-mono">
                  {(tools.denied as string[]).join(', ')}
                </span>
              </div>
            )}
            {skills.length > 0 && (
              <div>
                <span className="text-sera-text-muted">Skills: </span>
                <span className="text-sera-text font-mono">{skills.join(', ')}</span>
              </div>
            )}
          </div>
        </section>
      )}

      {/* Resolved Capabilities */}
      {Object.keys(resolvedCaps).length > 0 && (
        <section className="sera-card-static p-4">
          <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
            Resolved Capabilities
          </h3>
          <div className="space-y-1 text-xs">
            {Object.entries(resolvedCaps).map(([key, value]) => (
              <div key={key} className="mb-2">
                <span className="text-sera-text-muted text-[11px] uppercase tracking-wider">
                  {key}
                </span>
                {typeof value === 'object' && value !== null ? (
                  <div className="mt-1 ml-2 space-y-0.5">
                    {Object.entries(value as Record<string, unknown>).map(([k, v]) => (
                      <div key={k} className="flex items-start gap-2">
                        <span className="text-sera-text-dim min-w-[120px]">{k}:</span>
                        <span className="text-sera-text font-mono break-all">
                          {Array.isArray(v)
                            ? v.join(', ')
                            : typeof v === 'object'
                              ? JSON.stringify(v, null, 2)
                              : String(v)}
                        </span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <span className="text-sera-text font-mono ml-2">{String(value)}</span>
                )}
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Container / Runtime */}
      <section className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Runtime
        </h3>
        <div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
          <Field label="Status" value={(inst.status as string) || '—'} />
          <Field
            label="Container ID"
            value={(inst.container_id as string)?.slice(0, 12) || '—'}
            mono
          />
          <Field
            label="Created"
            value={inst.created_at ? new Date(inst.created_at as string).toLocaleString() : '—'}
          />
          <Field
            label="Updated"
            value={inst.updated_at ? new Date(inst.updated_at as string).toLocaleString() : '—'}
          />
          {typeof inst.last_heartbeat_at === 'string' && (
            <Field
              label="Last Heartbeat"
              value={new Date(inst.last_heartbeat_at as string).toLocaleString()}
            />
          )}
        </div>
      </section>

      {/* Raw JSON toggle */}
      <div>
        <button
          onClick={() => setShowRaw((p) => !p)}
          className="text-xs text-sera-text-dim hover:text-sera-text transition-colors"
        >
          {showRaw ? 'Hide' : 'Show'} raw JSON
        </button>
        {showRaw && (
          <pre className="sera-card-static p-4 mt-2 text-xs font-mono text-sera-text leading-relaxed overflow-x-auto whitespace-pre">
            {JSON.stringify(instance, null, 2)}
          </pre>
        )}
      </div>
    </div>
  );
}

function Field({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="text-sera-text-muted min-w-[120px]">{label}</span>
      <span className={cn('text-sera-text', mono && 'font-mono')}>{value}</span>
    </div>
  );
}
