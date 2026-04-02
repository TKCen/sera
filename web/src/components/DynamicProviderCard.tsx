import { useState } from 'react';
import {
  XCircle,
  ChevronDown,
  ChevronUp,
  Radio,
  Activity,
  Trash2,
  ExternalLink,
} from 'lucide-react';
import type { DynamicProviderConfig, DynamicProviderStatus } from '@/hooks/useProviders';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';

export function DynamicProviderCard({
  provider,
  status,
  onRemove,
}: {
  provider: DynamicProviderConfig;
  status?: DynamicProviderStatus;
  onRemove: (id: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const isHealthy = status?.status === 'ok';

  return (
    <div
      className={`sera-card-static overflow-hidden ${
        isHealthy
          ? 'border-sera-success/30'
          : 'border-sera-error/30 shadow-[0_0_15px_rgba(255,82,82,0.05)]'
      }`}
    >
      <button
        onClick={() => setExpanded((e) => !e)}
        className="w-full p-4 flex items-center justify-between hover:bg-sera-surface-hover transition-colors"
      >
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-lg flex items-center justify-center bg-amber-500/10 border border-amber-500/20">
            <Radio size={16} className="text-amber-400" />
          </div>
          <div className="text-left">
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold text-sera-text">{provider.name}</h3>
              {isHealthy ? (
                <Badge variant="success" className="text-[9px] px-1.5 py-0">
                  Online
                </Badge>
              ) : (
                <Badge variant="error" className="text-[9px] px-1.5 py-0">
                  Offline
                </Badge>
              )}
            </div>
            <p className="text-[11px] text-sera-text-muted mt-0.5 font-mono select-all">
              {provider.baseUrl}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <span className="text-[11px] text-sera-text-dim px-2 py-0.5 rounded-full bg-sera-bg/80 border border-sera-border">
            {status?.discoveredModels?.length ?? 0} models
          </span>
          {expanded ? (
            <ChevronUp size={14} className="text-sera-text-dim" />
          ) : (
            <ChevronDown size={14} className="text-sera-text-dim" />
          )}
        </div>
      </button>

      {expanded && (
        <div className="border-t border-sera-border p-4 space-y-4 bg-sera-bg/50 animate-in slide-in-from-top-2 duration-200">
          <div className="space-y-3">
            <div className="flex justify-between items-center px-1">
              <span className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider flex items-center gap-1.5">
                <Activity size={10} /> Discovery Status
              </span>
              <span className="text-[11px] text-sera-text-muted font-mono bg-sera-surface/80 px-2 py-0.5 rounded border border-sera-border">
                {status?.lastCheck ? new Date(status.lastCheck).toLocaleTimeString() : 'Never'}
              </span>
            </div>

            {!isHealthy && status?.error && (
              <div className="flex items-start gap-2 p-3 rounded-lg bg-sera-error/5 border border-sera-error/20 text-sera-error text-[11px] leading-relaxed">
                <XCircle size={14} className="mt-0.5 shrink-0" />
                <span>{status.error}</span>
              </div>
            )}

            {isHealthy && (
              <div className="space-y-2">
                <span className="text-[11px] text-sera-text-dim px-1 block">Live Models:</span>
                <div className="flex flex-wrap gap-2 p-2 rounded-lg bg-sera-bg/30 border border-sera-border/50">
                  {status?.discoveredModels?.map((m) => (
                    <span
                      key={m}
                      className="px-2 py-1 rounded border border-sera-border bg-sera-surface/50 text-[10px] text-sera-text-muted font-mono hover:border-sera-accent/30 hover:text-sera-text transition-colors cursor-default"
                    >
                      {m}
                    </span>
                  ))}
                  {status?.discoveredModels?.length === 0 && (
                    <span className="text-[11px] text-sera-text-dim italic px-2 py-1">
                      No models found — start them in LM Studio
                    </span>
                  )}
                </div>
              </div>
            )}
          </div>

          <div className="flex gap-2 border-t border-sera-border pt-4 mt-2">
            <Button
              variant="outline"
              size="sm"
              className="flex-1 text-xs h-9 bg-sera-error/5 hover:bg-sera-error/10 text-sera-error border-sera-error/20 gap-2"
              onClick={() => onRemove(provider.id)}
            >
              <Trash2 size={13} /> Remove
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="flex-1 text-xs h-9 gap-2"
              onClick={() => window.open(provider.baseUrl, '_blank')}
            >
              <ExternalLink size={13} /> API Info
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
