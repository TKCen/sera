import { useState } from 'react';
import {
  ChevronDown,
  ChevronRight,
  Shield,
  Server,
  Zap,
  Bot,
} from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import type { ToolInfo } from '@/lib/api/types';

export function ToolCard({ tool }: { tool: ToolInfo }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="sera-card p-4 flex flex-col gap-2">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="font-mono text-sm font-medium text-sera-text">{tool.id}</span>
            <Badge variant={tool.source === 'builtin' ? 'default' : 'accent'}>{tool.source}</Badge>
          </div>
          {tool.server && (
            <span className="text-[10px] text-sera-text-dim flex items-center gap-1 mt-0.5">
              <Server size={9} /> {tool.server}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1.5 flex-shrink-0">
          {tool.minTier != null && (
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface-hover text-sera-text-dim flex items-center gap-1">
              <Shield size={9} /> Tier {tool.minTier}+
            </span>
          )}
        </div>
      </div>

      {tool.description && (
        <p className="text-xs text-sera-text-muted line-clamp-2">{tool.description}</p>
      )}

      {tool.capabilityRequired && (
        <div className="flex items-center gap-1 text-[10px] text-amber-400">
          <Zap size={9} /> Requires{' '}
          <code className="bg-sera-bg px-1 rounded">{tool.capabilityRequired}</code>
        </div>
      )}

      {/* Parameters */}
      {tool.parameters && tool.parameters.length > 0 && (
        <div>
          <button
            onClick={() => setExpanded(!expanded)}
            className="text-[10px] text-sera-text-dim hover:text-sera-text flex items-center gap-1 transition-colors"
          >
            {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
            {tool.parameters.length} parameter{tool.parameters.length !== 1 ? 's' : ''}
          </button>
          {expanded && (
            <div className="mt-1.5 space-y-1 pl-3 border-l border-sera-border/50">
              {tool.parameters.map((p) => (
                <div key={p.name} className="text-[10px]">
                  <span className="font-mono text-sera-accent">{p.name}</span>
                  <span className="text-sera-text-dim ml-1">({p.type})</span>
                  {p.required && <span className="text-amber-400 ml-1">*</span>}
                  {p.description && (
                    <span className="text-sera-text-dim ml-1.5">— {p.description}</span>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Used by */}
      {tool.usedBy && tool.usedBy.length > 0 && (
        <div className="flex items-center gap-1.5 flex-wrap mt-1">
          <Bot size={10} className="text-sera-text-dim flex-shrink-0" />
          {tool.usedBy.map((agent) => (
            <span
              key={agent}
              className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface-hover text-sera-text-muted"
            >
              {agent}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
