import { useAgentTools } from '@/hooks/useAgents';
import { TabLoading } from '@/components/AgentDetailTabLoading';
import { Badge } from '@/components/ui/badge';
import { Wrench, AlertTriangle, ShieldCheck } from 'lucide-react';

// Built-in agent-runtime tools that are natively supported
const BUILTIN_RUNTIME_TOOLS = new Set([
  // core
  'tool-search',
  'skill-search',
  // memory
  'knowledge-store',
  'knowledge-query',
  // filesystem
  'file-read',
  'read_file',
  'file-write',
  'file-list',
  'file-delete',
  'glob',
  'grep',
  // web
  'web-fetch',
  'http-request',
  // compute
  'code-eval',
  'shell-exec',
  'pdf-read',
  'image-view',
  // orchestration
  'spawn-subagent',
  'run-tool',
]);

export function AgentDetailToolsTab({ id }: { id: string }) {
  const { data, isLoading, isError, error } = useAgentTools(id);

  if (isLoading) return <TabLoading />;
  if (isError) {
    return (
      <div className="p-6 text-sm text-sera-error">
        Failed to load tools: {error instanceof Error ? error.message : 'Unknown error'}
      </div>
    );
  }

  const { available = [], unavailable = [] } = data || {};

  // Separate built-in tools from truly unavailable tools
  const builtinTools = unavailable.filter((toolId) => BUILTIN_RUNTIME_TOOLS.has(toolId));
  const missingTools = unavailable.filter((toolId) => !BUILTIN_RUNTIME_TOOLS.has(toolId));

  return (
    <div className="p-6 space-y-8 max-w-4xl">
      {/* Available Tools */}
      <section>
        <div className="flex items-center gap-2 mb-4">
          <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
            <ShieldCheck size={14} className="text-sera-success" />
            Available Tools
          </h3>
          <Badge variant="default" className="text-[10px] h-4">
            {available.length + builtinTools.length} Available
          </Badge>
        </div>

        {available.length === 0 && builtinTools.length === 0 ? (
          <div className="sera-card-static p-8 text-center text-sm text-sera-text-muted">
            No registered tools available for this agent.
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {/* MCP/Custom registered tools */}
            {available.map((tool) => (
              <div
                key={tool.id}
                className="sera-card-static p-3 hover:bg-sera-surface-hover transition-colors border-sera-border/40"
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="flex items-center gap-2 min-w-0">
                    <Wrench size={13} className="text-sera-accent shrink-0" />
                    <span className="text-sm font-medium text-sera-text truncate font-mono">
                      {tool.id}
                    </span>
                  </div>
                  <Badge variant="default" className="text-[10px] h-4 shrink-0">
                    {tool.source}
                  </Badge>
                </div>
                {tool.description && (
                  <p className="text-xs text-sera-text-muted mt-2 line-clamp-2 leading-relaxed">
                    {tool.description}
                  </p>
                )}
                {tool.server && (
                  <div className="mt-2 text-[10px] text-sera-text-dim flex items-center gap-1">
                    <span>Server:</span>
                    <span className="font-mono">{tool.server}</span>
                  </div>
                )}
              </div>
            ))}

            {/* Built-in agent-runtime tools */}
            {builtinTools.map((toolId) => (
              <div
                key={toolId}
                className="sera-card-static p-3 hover:bg-sera-surface-hover transition-colors border-sera-border/40"
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="flex items-center gap-2 min-w-0">
                    <Wrench size={13} className="text-sera-accent shrink-0" />
                    <span className="text-sm font-medium text-sera-text truncate font-mono">
                      {toolId}
                    </span>
                  </div>
                  <Badge variant="default" className="text-[10px] h-4 shrink-0">
                    Built-in
                  </Badge>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* Unavailable Tools */}
      {missingTools.length > 0 && (
        <section>
          <div className="flex items-center gap-2 mb-4">
            <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
              <AlertTriangle size={14} className="text-sera-warning" />
              Unavailable Tools
            </h3>
            <Badge variant="warning" className="text-[10px] h-4">
              {missingTools.length} Missing
            </Badge>
          </div>
          <p className="text-xs text-sera-text-muted mb-4 leading-relaxed">
            These tools are requested in the agent&apos;s manifest or template but are not currently
            registered in the system. They may belong to an MCP server that is offline or
            misconfigured.
          </p>
          <div className="sera-card-static divide-y divide-sera-border/30 overflow-hidden">
            {missingTools.map((toolId) => (
              <div
                key={toolId}
                className="px-4 py-2.5 flex items-center justify-between gap-3 bg-sera-warning/5"
              >
                <span className="text-xs font-mono text-sera-warning font-medium">{toolId}</span>
                <span className="text-[10px] text-sera-warning/70 uppercase font-bold tracking-wider">
                  Unregistered
                </span>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
