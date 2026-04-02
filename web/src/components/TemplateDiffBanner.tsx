import { useState } from 'react';
import { ChevronDown, ChevronUp, RefreshCw, CheckCircle } from 'lucide-react';
import { toast } from 'sonner';
import { useTemplateDiff, useApplyTemplateUpdate, useSkipTemplateUpdate } from '@/hooks/useAgents';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { TemplateDiffChange } from '@/lib/api/types';

interface TemplateDiffBannerProps {
  agentId: string;
}

function impactBadgeClass(impact: TemplateDiffChange['impact']): string {
  switch (impact) {
    case 'breaking':
      return 'bg-sera-error/20 text-sera-error border border-sera-error/30';
    case 'permission':
      return 'bg-yellow-500/15 text-yellow-400 border border-yellow-500/30';
    case 'resource':
      return 'bg-orange-500/15 text-orange-400 border border-orange-500/30';
    default:
      return 'bg-sera-accent-soft text-sera-accent border border-sera-accent/30';
  }
}

function typeBadgeClass(type: TemplateDiffChange['type']): string {
  switch (type) {
    case 'added':
      return 'bg-sera-success/15 text-sera-success border border-sera-success/30';
    case 'removed':
      return 'bg-sera-error/15 text-sera-error border border-sera-error/30';
    default:
      return 'bg-sera-text-dim/15 text-sera-text-muted border border-sera-border';
  }
}

export function TemplateDiffBanner({ agentId }: TemplateDiffBannerProps) {
  const { data: diff, isLoading } = useTemplateDiff(agentId);
  const applyUpdate = useApplyTemplateUpdate(agentId);
  const skipUpdate = useSkipTemplateUpdate(agentId);
  const [expanded, setExpanded] = useState(false);

  if (isLoading || !diff || !diff.hasChanges) return null;

  const handleApplyAll = async () => {
    try {
      await applyUpdate.mutateAsync(undefined);
      toast.success('Template update applied');
    } catch {
      toast.error('Failed to apply template update');
    }
  };

  const handleApplyPartial = async (path: string) => {
    try {
      await applyUpdate.mutateAsync([path]);
      toast.success(`Applied update for ${path}`);
    } catch {
      toast.error(`Failed to apply update for ${path}`);
    }
  };

  const handleSkip = async () => {
    try {
      await skipUpdate.mutateAsync();
      toast.success('Template update dismissed');
    } catch {
      toast.error('Failed to dismiss template update');
    }
  };

  return (
    <div className="mx-6 mt-4 rounded-lg border border-yellow-500/40 bg-yellow-500/10">
      {/* Banner header */}
      <div className="flex items-center gap-3 px-4 py-3">
        <RefreshCw size={14} className="text-yellow-400 flex-shrink-0" />
        <div className="flex-1 min-w-0">
          <span className="text-sm font-medium text-yellow-300">Template update available</span>
          <span className="ml-2 text-xs text-yellow-400/80">
            {diff.changes.length > 0
              ? `${diff.changes.length} change${diff.changes.length !== 1 ? 's' : ''}`
              : 'Template was updated since last apply'}
          </span>
        </div>
        <div className="flex items-center gap-2 flex-shrink-0">
          {diff.changes.length > 0 && (
            <button
              onClick={() => setExpanded((v) => !v)}
              className="flex items-center gap-1 text-xs text-yellow-400 hover:text-yellow-300 transition-colors"
            >
              {expanded ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
              {expanded ? 'Hide' : 'Show'} changes
            </button>
          )}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              void handleSkip();
            }}
            disabled={skipUpdate.isPending || applyUpdate.isPending}
            className="text-yellow-400 hover:bg-yellow-500/10"
          >
            {skipUpdate.isPending ? 'Skipping…' : 'Skip'}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              void handleApplyAll();
            }}
            disabled={applyUpdate.isPending || skipUpdate.isPending}
          >
            {applyUpdate.isPending ? 'Applying…' : 'Apply All'}
          </Button>
        </div>
      </div>

      {/* Expanded change list */}
      {expanded && diff.changes.length > 0 && (
        <div className="border-t border-yellow-500/20 divide-y divide-yellow-500/10">
          {diff.changes.map((change, i) => (
            <div key={i} className="px-4 py-2.5 flex items-start gap-3 group">
              <code className="text-xs font-mono text-sera-text flex-1 min-w-0 break-all">
                {change.path || '(root)'}
              </code>
              <div className="flex items-center gap-1.5 flex-shrink-0">
                <span
                  className={cn(
                    'text-xs px-1.5 py-0.5 rounded font-medium',
                    typeBadgeClass(change.type)
                  )}
                >
                  {change.type}
                </span>
                <span
                  className={cn(
                    'text-xs px-1.5 py-0.5 rounded font-medium',
                    impactBadgeClass(change.impact)
                  )}
                >
                  {change.impact}
                </span>
              </div>
              {(change.oldValue !== undefined || change.newValue !== undefined) && (
                <div className="text-xs text-sera-text-muted font-mono flex-shrink-0 max-w-xs truncate">
                  {change.type === 'changed' && (
                    <>
                      <span className="text-sera-error line-through mr-1">
                        {JSON.stringify(change.oldValue)}
                      </span>
                      <span className="text-sera-success">{JSON.stringify(change.newValue)}</span>
                    </>
                  )}
                  {change.type === 'added' && (
                    <span className="text-sera-success">{JSON.stringify(change.newValue)}</span>
                  )}
                  {change.type === 'removed' && (
                    <span className="text-sera-error line-through">
                      {JSON.stringify(change.oldValue)}
                    </span>
                  )}
                </div>
              )}
              <button
                onClick={() => {
                  void handleApplyPartial(change.path);
                }}
                disabled={applyUpdate.isPending}
                className="flex-shrink-0 text-yellow-400 hover:text-yellow-300 opacity-0 group-hover:opacity-100 transition-all p-1"
                title="Apply this change only"
              >
                <CheckCircle size={14} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
