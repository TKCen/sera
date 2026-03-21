import { Link } from 'react-router';
import { LayoutTemplate, Plus, Lock } from 'lucide-react';
import { useTemplates } from '@/hooks/useTemplates';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';

export default function TemplatesPage() {
  const { data: templates, isLoading } = useTemplates();

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">Templates</h1>
      </div>
      <p className="text-sm text-sera-text-muted mb-6">
        Agent templates are reusable blueprints. Create an agent instance from any template below.
      </p>

      {isLoading ? (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-40 rounded-xl" />
          ))}
        </div>
      ) : !templates?.length ? (
        <EmptyState
          icon={<LayoutTemplate size={24} />}
          title="No templates"
          description="No agent templates have been loaded. Place template YAML files in templates/builtin/ or templates/custom/."
        />
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {templates.map((t) => {
            const spec = t.spec as Record<string, unknown> | undefined;
            const identity = spec?.identity as Record<string, string> | undefined;
            const lifecycle = spec?.lifecycle as Record<string, string> | undefined;
            const sandbox = spec?.sandboxBoundary as string | undefined;

            return (
              <div key={t.name} className="sera-card p-4 flex flex-col gap-3">
                <div className="flex items-start justify-between">
                  <div>
                    <div className="font-medium text-sm text-sera-text">
                      {t.displayName ?? t.name}
                    </div>
                    <span className="text-xs text-sera-text-dim">{t.name}</span>
                  </div>
                  <div className="flex items-center gap-1.5">
                    {t.builtin && (
                      <Badge variant="default" className="gap-1">
                        <Lock size={9} /> Built-in
                      </Badge>
                    )}
                    {sandbox && <Badge variant="accent">{sandbox}</Badge>}
                  </div>
                </div>

                {(t.description ?? identity?.role) && (
                  <p className="text-xs text-sera-text-muted line-clamp-3">
                    {t.description ?? identity?.role}
                  </p>
                )}

                <div className="flex items-center gap-2 mt-auto">
                  {lifecycle?.mode && (
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface-hover text-sera-text-dim">
                      {lifecycle.mode}
                    </span>
                  )}
                </div>

                <Button asChild size="sm" className="mt-1">
                  <Link to={`/agents/new?template=${encodeURIComponent(t.name)}`}>
                    <Plus size={12} />
                    Create Agent
                  </Link>
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
