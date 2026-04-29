import { useQuery } from '@tanstack/react-query';
import { useNavigate } from 'react-router';
import { getAgents, ApiError } from '@/lib/api';

export function AgentsListView() {
  const navigate = useNavigate();

  const { data, isLoading, error } = useQuery({
    queryKey: ['agents'],
    queryFn: getAgents,
  });

  if (isLoading) {
    return (
      <div className="p-6 text-sm text-zinc-400">Loading agents…</div>
    );
  }

  if (error) {
    const msg = error instanceof ApiError ? error.message : (error as Error).message;
    return (
      <div className="p-6 text-sm text-red-400">Failed to load agents: {msg}</div>
    );
  }

  return (
    <div className="space-y-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Agents</h1>
        <p className="text-sm text-zinc-500">Registered agent manifests.</p>
      </div>

      {data && data.length === 0 ? (
        <p className="text-sm text-zinc-500">No agents registered.</p>
      ) : (
        <ul className="space-y-2">
          {data?.map((agent) => (
            <li key={agent.name}>
              <button
                type="button"
                onClick={() => void navigate(`/agents/${encodeURIComponent(agent.name)}`)}
                className="w-full rounded-lg border border-zinc-800 bg-zinc-900 p-4 text-left hover:border-zinc-700 hover:bg-zinc-800"
              >
                <div className="font-medium text-zinc-100">{agent.name}</div>
                <div className="mt-1 text-xs text-zinc-500">
                  provider: {agent.provider || '—'}
                  {agent.model ? ` · model: ${agent.model}` : ''}
                  {` · tools: ${agent.has_tools ? 'yes' : 'no'}`}
                </div>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
