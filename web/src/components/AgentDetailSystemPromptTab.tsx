import { useAgentSystemPrompt } from '@/hooks/useAgents';
import { TabLoading } from '@/components/AgentDetailTabLoading';

export function AgentDetailSystemPromptTab({ id }: { id: string }) {
  const { data, isLoading } = useAgentSystemPrompt(id);

  if (isLoading) return <TabLoading />;

  return (
    <div className="p-6 max-w-4xl">
      <h3 className="text-sm font-semibold text-sera-text mb-3">Resolved System Prompt</h3>
      <p className="text-xs text-sera-text-muted mb-4">
        This is the full system prompt sent to the LLM on each request, built from the agent&apos;s
        template identity, tools, and configuration.
      </p>
      <pre className="sera-card-static p-4 text-xs font-mono text-sera-text leading-relaxed overflow-auto whitespace-pre-wrap max-h-[70vh]">
        {data?.prompt || 'Unable to generate system prompt.'}
      </pre>
    </div>
  );
}
