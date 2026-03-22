import { useMemo } from 'react';
import { useParams, Link } from 'react-router';
import { ArrowLeft } from 'lucide-react';
import { useAgent } from '@/hooks/useAgents';
import { Spinner } from '@/components/ui/spinner';
import { AgentForm } from '@/components/AgentForm';
import type { AgentFormInitialValues } from '@/components/AgentForm';

export default function AgentEditPage() {
  const { id = '' } = useParams<{ id: string }>();
  const { data: instance, isLoading } = useAgent(id);

  const initialValues: AgentFormInitialValues | undefined = useMemo(() => {
    if (!instance) return undefined;
    const overrides = (instance.overrides ?? {}) as Record<string, unknown>;
    const modelOv = overrides.model as Record<string, unknown> | undefined;
    const resourcesOv = overrides.resources as Record<string, unknown> | undefined;
    const permissionsOv = overrides.permissions as Record<string, unknown> | undefined;
    const toolsOv = overrides.tools as Record<string, unknown> | undefined;

    return {
      templateRef: instance.template_ref,
      name: instance.name,
      displayName: instance.display_name ?? '',
      circle: instance.circle ?? '',
      lifecycleMode: (instance.lifecycle_mode as 'persistent' | 'ephemeral') ?? 'persistent',
      modelName: (modelOv?.name as string) ?? '',
      modelProvider: (modelOv?.provider as string) ?? '',
      temperature: (modelOv?.temperature as number) ?? 0.7,
      sandboxBoundary: (overrides.sandboxBoundary as string) ?? 'tier-2',
      tokensPerHour: (resourcesOv?.maxLlmTokensPerHour as number) ?? 100000,
      tokensPerDay: (resourcesOv?.maxLlmTokensPerDay as number) ?? 1000000,
      canExec: (permissionsOv?.canExec as boolean) ?? false,
      canSpawnSubagents: (permissionsOv?.canSpawnSubagents as boolean) ?? false,
      toolsAllowed: Array.isArray(toolsOv?.allowed) ? (toolsOv.allowed as string[]) : [],
      toolsDenied: Array.isArray(toolsOv?.denied) ? (toolsOv.denied as string[]) : [],
      skills: Array.isArray(overrides.skills) ? (overrides.skills as string[]) : [],
    };
  }, [instance]);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-20">
        <Spinner />
      </div>
    );
  }

  if (!instance) {
    return (
      <div className="p-6 max-w-2xl">
        <p className="text-sm text-sera-text-muted">Agent instance not found.</p>
      </div>
    );
  }

  return (
    <div className="p-6 max-w-2xl">
      <Link
        to={`/agents/${id}`}
        className="inline-flex items-center gap-1.5 text-xs text-sera-text-muted hover:text-sera-text mb-6 transition-colors"
      >
        <ArrowLeft size={12} /> Back
      </Link>
      <div className="sera-page-header mb-6">
        <h1 className="sera-page-title">Edit Agent</h1>
        <p className="text-sm text-sera-text-muted mt-1">
          Editing <strong>{instance.name}</strong> — template: {instance.template_ref}
        </p>
      </div>
      <AgentForm mode="edit" instanceId={id} initialValues={initialValues} />
    </div>
  );
}
