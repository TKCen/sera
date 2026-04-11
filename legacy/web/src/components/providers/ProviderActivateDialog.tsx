import { useState, useId } from 'react';
import { Plus, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { useAddProvider } from '@/hooks/useProviders';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';
import type { ProviderTemplate } from '@/lib/api/providers';

export function ProviderActivateDialog({
  template,
  onClose,
}: {
  template: ProviderTemplate | null;
  onClose: () => void;
}) {
  const [apiKey, setApiKey] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [selectedModels, setSelectedModels] = useState<Set<string>>(new Set());
  const addProvider = useAddProvider();
  const apiKeyId = useId();
  const baseUrlId = useId();

  if (!template) return null;

  const allSelected = selectedModels.size === template.models.length;

  const handleActivate = async () => {
    if (!apiKey && !template.baseUrl) {
      toast.error('API key is required');
      return;
    }
    const models = selectedModels.size > 0 ? Array.from(selectedModels) : template.models;
    let successCount = 0;
    for (const modelName of models) {
      try {
        await addProvider.mutateAsync({
          modelName,
          api: template.api,
          provider: template.provider,
          baseUrl: baseUrl || template.baseUrl,
          apiKey: apiKey || undefined,
          apiKeyEnvVar: template.apiKeyEnvVar,
        });
        successCount++;
      } catch {
        toast.error(`Failed to add ${modelName}`);
      }
    }
    if (successCount > 0) {
      toast.success(
        `Added ${successCount} model${successCount > 1 ? 's' : ''} from ${template.displayName}`
      );
    }
    onClose();
  };

  return (
    <Dialog open={!!template} onOpenChange={(o: boolean) => !o && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Activate {template.displayName}</DialogTitle>
          <DialogDescription>{template.description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4 mt-2">
          <div>
            <label htmlFor={apiKeyId} className="text-xs text-sera-text-muted block mb-1">
              API Key ({template.apiKeyEnvVar})
            </label>
            <Input
              id={apiKeyId}
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={`Paste your ${template.provider} API key…`}
            />
          </div>
          {!template.baseUrl && (
            <div>
              <label htmlFor={baseUrlId} className="text-xs text-sera-text-muted block mb-1">
                Base URL (optional)
              </label>
              <Input
                id={baseUrlId}
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://api.example.com/v1"
              />
            </div>
          )}
          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="text-xs text-sera-text-muted">
                Models ({selectedModels.size || template.models.length} selected)
              </label>
              <button
                type="button"
                className="text-[10px] text-sera-accent hover:underline"
                onClick={() =>
                  setSelectedModels(allSelected ? new Set() : new Set(template.models))
                }
              >
                {allSelected ? 'Deselect all' : 'Select all'}
              </button>
            </div>
            <div className="max-h-40 overflow-y-auto space-y-1 border border-sera-border rounded-lg p-2">
              {template.models.map((m) => (
                <label
                  key={m}
                  className="flex items-center gap-2 text-xs text-sera-text cursor-pointer"
                >
                  <input
                    type="checkbox"
                    checked={selectedModels.has(m) || selectedModels.size === 0}
                    onChange={(e) => {
                      const next = new Set(
                        selectedModels.size === 0 ? template.models : selectedModels
                      );
                      if (e.target.checked) next.add(m);
                      else next.delete(m);
                      setSelectedModels(next);
                    }}
                    className="rounded"
                  />
                  <span className="font-mono">{m}</span>
                </label>
              ))}
            </div>
          </div>
          <div className="flex gap-3 justify-end">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button
              size="sm"
              onClick={() => void handleActivate()}
              disabled={addProvider.isPending}
            >
              {addProvider.isPending ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <Plus size={14} />
              )}
              Activate
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
