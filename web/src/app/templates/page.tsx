import { useState, useMemo } from 'react';
import { Link } from 'react-router';
import { LayoutTemplate, Plus, Lock, X, Pencil, Trash2 } from 'lucide-react';
import {
  useTemplates,
  useCreateTemplate,
  useUpdateTemplate,
  useDeleteTemplate,
} from '@/hooks/useTemplates';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
  DialogFooter,
} from '@/components/ui/dialog';
import { toast } from 'sonner';
import yaml from 'js-yaml';

interface TemplateData {
  name: string;
  displayName?: string | null;
  description?: string | null;
  builtin?: boolean;
  spec?: Record<string, unknown>;
}

function TemplateDetailDialog({
  template,
  onClose,
  onEdit,
}: {
  template: TemplateData | null;
  onClose: () => void;
  onEdit: (t: TemplateData) => void;
}) {
  const deleteTemplate = useDeleteTemplate();
  const [showConfirmDelete, setShowConfirmDelete] = useState(false);

  if (!template) return null;

  const handleDelete = async () => {
    try {
      await deleteTemplate.mutateAsync(template.name);
      toast.success(`Template ${template.name} deleted`);
      onClose();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to delete template');
    }
  };

  const spec = template.spec ?? {};
  const identity = spec.identity as Record<string, string> | undefined;
  const model = spec.model as Record<string, unknown> | undefined;
  const lifecycle = spec.lifecycle as Record<string, string> | undefined;
  const sandbox = spec.sandboxBoundary as string | undefined;
  const tools = spec.tools as string[] | undefined;

  return (
    <Dialog open={!!template} onOpenChange={(o: boolean) => !o && onClose()}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{template.displayName ?? template.name}</DialogTitle>
          <DialogDescription>{template.name}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 mt-2">
          {/* Identity */}
          {identity && (
            <Section title="Identity">
              {identity.role && <Field label="Role" value={identity.role} />}
              {identity.personality && <Field label="Personality" value={identity.personality} />}
              {identity.goal && <Field label="Goal" value={identity.goal} />}
            </Section>
          )}

          {/* Model */}
          {model && (
            <Section title="Model">
              <Field label="Name" value={String(model.name ?? '—')} />
              {model.temperature !== undefined && (
                <Field label="Temperature" value={String(model.temperature)} />
              )}
              {model.maxTokens !== undefined && (
                <Field label="Max Tokens" value={String(model.maxTokens)} />
              )}
            </Section>
          )}

          {/* Lifecycle */}
          {lifecycle && (
            <Section title="Lifecycle">
              {lifecycle.mode && <Field label="Mode" value={lifecycle.mode} />}
              {lifecycle.idleTimeout && (
                <Field label="Idle Timeout" value={lifecycle.idleTimeout} />
              )}
            </Section>
          )}

          {/* Sandbox & Tools */}
          <div className="flex gap-6">
            {sandbox && (
              <Section title="Sandbox Boundary">
                <Badge variant="accent">{sandbox}</Badge>
              </Section>
            )}
            {tools && tools.length > 0 && (
              <Section title={`Tools (${tools.length})`}>
                <div className="flex flex-wrap gap-1">
                  {tools.map((t) => (
                    <Badge key={t} variant="default" className="text-[10px]">
                      {t}
                    </Badge>
                  ))}
                </div>
              </Section>
            )}
          </div>

          {/* Raw spec */}
          <Section title="Raw Spec">
            <pre className="text-[11px] font-mono text-sera-text-muted bg-sera-surface rounded-lg p-3 overflow-x-auto max-h-60 leading-relaxed">
              {JSON.stringify(spec, null, 2)}
            </pre>
          </Section>
        </div>

        <div className="flex gap-3 justify-between mt-4">
          <div className="flex gap-2">
            {!template.builtin && (
              <>
                <Button variant="outline" size="sm" onClick={() => onEdit(template)}>
                  <Pencil size={12} /> Edit
                </Button>
                <Button variant="danger" size="sm" onClick={() => setShowConfirmDelete(true)}>
                  <Trash2 size={12} /> Delete
                </Button>
              </>
            )}
          </div>
          <div className="flex gap-2">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                <X size={12} /> Close
              </Button>
            </DialogClose>
            <Button asChild size="sm">
              <Link to={`/agents/new?template=${encodeURIComponent(template.name)}`}>
                <Plus size={12} /> Create Agent
              </Link>
            </Button>
          </div>
        </div>
      </DialogContent>

      <Dialog open={showConfirmDelete} onOpenChange={setShowConfirmDelete}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Template</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete template <strong>{template.name}</strong>? This action
              cannot be undone and may affect any existing agents using this template.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="ghost" size="sm" onClick={() => setShowConfirmDelete(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={handleDelete}
              disabled={deleteTemplate.isPending}
            >
              {deleteTemplate.isPending ? 'Deleting...' : 'Delete'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Dialog>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h3 className="text-[11px] font-bold uppercase tracking-wider text-sera-text-dim mb-1.5">
        {title}
      </h3>
      {children}
    </div>
  );
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex gap-2 text-xs mb-1">
      <span className="text-sera-text-muted min-w-[80px]">{label}:</span>
      <span className="text-sera-text">{value}</span>
    </div>
  );
}

function TemplateEditDialog({
  template,
  onClose,
}: {
  template: TemplateData | null | boolean; // true for new, TemplateData for edit
  onClose: () => void;
}) {
  const isNew = template === true;
  const initialData = useMemo(() => {
    if (typeof template === 'object' && template !== null) {
      return yaml.dump(template);
    }
    return yaml.dump({
      apiVersion: 'sera/v1',
      kind: 'Agent',
      metadata: {
        name: 'my-new-template',
        displayName: 'My New Template',
      },
      spec: {
        identity: {
          role: 'A helpful assistant',
        },
        model: {
          name: 'gpt-4o',
          temperature: 0.7,
        },
      },
    });
  }, [template]);

  const [code, setCode] = useState(initialData);
  const createTemplate = useCreateTemplate();
  const updateTemplate = useUpdateTemplate();

  const handleSave = async () => {
    try {
      const parsed = yaml.load(code) as any;
      if (!parsed.name && !parsed.metadata?.name) {
        toast.error('Template name is required (metadata.name)');
        return;
      }
      const name = parsed.name ?? parsed.metadata?.name;

      if (isNew) {
        await createTemplate.mutateAsync(parsed as any);
        toast.success(`Template ${name} created`);
      } else {
        await updateTemplate.mutateAsync({
          name: (template as TemplateData).name,
          template: parsed as any,
        });
        toast.success(`Template ${name} updated`);
      }
      onClose();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to save template');
    }
  };

  return (
    <Dialog open={!!template} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-3xl max-h-[90vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>{isNew ? 'Create Template' : 'Edit Template'}</DialogTitle>
          <DialogDescription>
            Templates are defined using YAML. Ensure your spec follows the Agent schema.
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 min-h-[400px] mt-4 relative">
          <textarea
            value={code}
            onChange={(e) => setCode(e.target.value)}
            className="w-full h-full p-4 font-mono text-xs bg-sera-surface border border-sera-border rounded-lg resize-none focus:outline-none focus:ring-1 focus:ring-sera-accent leading-relaxed"
            placeholder="Paste YAML template spec here..."
          />
        </div>

        <DialogFooter className="mt-4">
          <Button variant="ghost" size="sm" onClick={onClose}>
            Cancel
          </Button>
          <Button
            size="sm"
            onClick={handleSave}
            disabled={createTemplate.isPending || updateTemplate.isPending}
          >
            {createTemplate.isPending || updateTemplate.isPending ? 'Saving...' : 'Save Template'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export default function TemplatesPage() {
  const { data: templates, isLoading } = useTemplates();
  const [selectedTemplate, setSelectedTemplate] = useState<TemplateData | null>(null);
  const [editTemplate, setEditTemplate] = useState<TemplateData | null | boolean>(null);

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">Templates</h1>
        <Button size="sm" onClick={() => setEditTemplate(true)}>
          <Plus size={14} /> Create Template
        </Button>
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
              <div
                key={t.name}
                className="sera-card p-4 flex flex-col gap-3 cursor-pointer"
                role="button"
                tabIndex={0}
                onClick={() => setSelectedTemplate(t as TemplateData)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    setSelectedTemplate(t as TemplateData);
                  }
                }}
              >
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

                <Button
                  asChild
                  size="sm"
                  className="mt-1 relative z-10"
                  onClick={(e: React.MouseEvent) => e.stopPropagation()}
                >
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

      <TemplateDetailDialog
        template={selectedTemplate}
        onClose={() => setSelectedTemplate(null)}
        onEdit={(t) => {
          setSelectedTemplate(null);
          setEditTemplate(t);
        }}
      />

      <TemplateEditDialog template={editTemplate} onClose={() => setEditTemplate(null)} />
    </div>
  );
}
