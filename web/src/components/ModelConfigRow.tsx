import { useState } from 'react';
import { ChevronDown, Brain, Save } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { toast } from 'sonner';
import { useUpdateProviderConfig } from '@/hooks/useProviders';

export function ModelConfigRow({
  model,
  authStatus,
  contextWindow,
  maxTokens,
  contextStrategy,
  contextHighWaterMark,
  reasoning,
  onSaved,
}: {
  model: {
    modelName: string;
    provider?: string;
    api: string;
    baseUrl?: string;
    description?: string;
  };
  authStatus: string;
  contextWindow?: number;
  maxTokens?: number;
  contextStrategy?: string;
  contextHighWaterMark?: number;
  reasoning?: boolean;
  onSaved: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [localCtxWindow, setLocalCtxWindow] = useState(String(contextWindow ?? ''));
  const [localMaxTokens, setLocalMaxTokens] = useState(String(maxTokens ?? ''));
  const [localStrategy, setLocalStrategy] = useState(contextStrategy ?? 'sliding-window');
  const [localHighWater, setLocalHighWater] = useState(String(contextHighWaterMark ?? '0.8'));
  const [localReasoning, setLocalReasoning] = useState(reasoning ?? false);
  const updateProviderConfigMutation = useUpdateProviderConfig();

  const handleSave = async () => {
    try {
      const overrides: Record<string, unknown> = {};
      if (localCtxWindow) overrides.contextWindow = parseInt(localCtxWindow, 10);
      if (localMaxTokens) overrides.maxTokens = parseInt(localMaxTokens, 10);
      if (localStrategy) overrides.contextStrategy = localStrategy;
      if (localHighWater) overrides.contextHighWaterMark = parseFloat(localHighWater);
      overrides.reasoning = localReasoning;

      await updateProviderConfigMutation.mutateAsync({ modelName: model.modelName, config: overrides });
      toast.success(`Config updated for ${model.modelName}`);
      onSaved();
    } catch (err) {
      toast.error(`Failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  };

  return (
    <div className="border border-sera-border rounded-lg overflow-hidden">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between p-3 hover:bg-sera-surface/50 transition-colors text-left"
      >
        <div className="flex items-center gap-3">
          <span className="text-sera-text font-mono text-xs">{model.modelName}</span>
          <span className="text-sera-text-dim text-[10px]">{model.provider ?? ''}</span>
          <Badge
            variant={
              authStatus === 'configured'
                ? 'success'
                : authStatus === 'missing'
                  ? 'warning'
                  : 'default'
            }
            className="text-[9px]"
          >
            {authStatus === 'configured'
              ? 'Active'
              : authStatus === 'missing'
                ? 'Key missing'
                : 'Local'}
          </Badge>
          {reasoning && (
            <span className="flex items-center gap-0.5 text-[9px] text-violet-400">
              <Brain size={10} /> reasoning
            </span>
          )}
        </div>
        <ChevronDown
          size={14}
          className={`text-sera-text-dim transition-transform ${expanded ? 'rotate-180' : ''}`}
        />
      </button>

      {expanded && (
        <div className="border-t border-sera-border p-4 bg-sera-surface/30 space-y-3">
          <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
            <div>
              <label className="text-[10px] uppercase text-sera-text-dim block mb-1">
                Context Window
              </label>
              <input
                type="number"
                value={localCtxWindow}
                onChange={(e) => setLocalCtxWindow(e.target.value)}
                placeholder="128000"
                className="sera-input w-full text-xs"
              />
            </div>
            <div>
              <label className="text-[10px] uppercase text-sera-text-dim block mb-1">
                Max Tokens (output)
              </label>
              <input
                type="number"
                value={localMaxTokens}
                onChange={(e) => setLocalMaxTokens(e.target.value)}
                placeholder="4096"
                className="sera-input w-full text-xs"
              />
            </div>
            <div>
              <label className="text-[10px] uppercase text-sera-text-dim block mb-1">
                Context Strategy
              </label>
              <select
                value={localStrategy}
                onChange={(e) => setLocalStrategy(e.target.value)}
                className="sera-input w-full text-xs"
              >
                <option value="sliding-window">Sliding Window</option>
                <option value="summarize">Summarize</option>
                <option value="truncate">Truncate</option>
              </select>
            </div>
            <div>
              <label className="text-[10px] uppercase text-sera-text-dim block mb-1">
                High Water Mark
              </label>
              <input
                type="number"
                step="0.05"
                min="0.5"
                max="0.95"
                value={localHighWater}
                onChange={(e) => setLocalHighWater(e.target.value)}
                className="sera-input w-full text-xs"
              />
            </div>
            <div className="flex items-center gap-2 pt-4">
              <input
                type="checkbox"
                checked={localReasoning}
                onChange={(e) => setLocalReasoning(e.target.checked)}
                id={`reasoning-${model.modelName}`}
                className="accent-sera-accent"
              />
              <label htmlFor={`reasoning-${model.modelName}`} className="text-xs text-sera-text">
                Reasoning model
              </label>
            </div>
          </div>
          <div className="flex justify-end pt-1">
            <Button size="sm" onClick={handleSave} disabled={updateProviderConfigMutation.isPending}>
              <Save size={12} className="mr-1" />
              {updateProviderConfigMutation.isPending ? 'Saving...' : 'Save Config'}
            </Button>
          </div>
          <p className="text-[10px] text-sera-text-dim">
            Context window controls SERA's compaction threshold. The actual model context depends on
            your provider's settings (e.g., LM Studio model configuration).
          </p>
        </div>
      )}
    </div>
  );
}
