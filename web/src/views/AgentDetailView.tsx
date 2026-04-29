import { useQuery } from '@tanstack/react-query';
import { useParams, Link } from 'react-router';
import { getAgent, ApiError } from '@/lib/api';

export function AgentDetailView() {
  const { id } = useParams<{ id: string }>();

  const { data, isLoading, error } = useQuery({
    queryKey: ['agent', id],
    queryFn: () => getAgent(id!),
    enabled: !!id,
  });

  const backLink = (
    <Link to="/agents" className="text-sm text-zinc-400 hover:text-zinc-200">
      ← Back to agents
    </Link>
  );

  if (isLoading) {
    return (
      <div className="space-y-4 p-6">
        {backLink}
        <p className="text-sm text-zinc-400">Loading…</p>
      </div>
    );
  }

  if (error) {
    if (error instanceof ApiError && error.status === 404) {
      return (
        <div className="space-y-4 p-6">
          {backLink}
          <p className="text-sm text-zinc-400">Agent not found.</p>
        </div>
      );
    }
    const msg = error instanceof ApiError ? error.message : (error as Error).message;
    return (
      <div className="space-y-4 p-6">
        {backLink}
        <p className="text-sm text-red-400">Failed to load agent: {msg}</p>
      </div>
    );
  }

  const scalar: [string, string][] = [];
  const nested: [string, unknown][] = [];

  if (data) {
    for (const [key, value] of Object.entries(data)) {
      if (value === null || value === undefined) {
        scalar.push([key, '—']);
      } else if (typeof value === 'object') {
        nested.push([key, value]);
      } else {
        scalar.push([key, String(value)]);
      }
    }
  }

  return (
    <div className="space-y-6 p-6">
      {backLink}
      <div>
        <h1 className="text-2xl font-semibold">{data?.name ?? id}</h1>
        <p className="text-sm text-zinc-500">Agent detail</p>
      </div>

      {scalar.length > 0 && (
        <dl className="divide-y divide-zinc-800 rounded-lg border border-zinc-800">
          {scalar.map(([key, value]) => (
            <div key={key} className="flex gap-4 px-4 py-3">
              <dt className="w-32 shrink-0 text-xs uppercase tracking-wider text-zinc-500">{key}</dt>
              <dd className="text-sm text-zinc-100">{value}</dd>
            </div>
          ))}
        </dl>
      )}

      {nested.map(([key, value]) => (
        <div key={key}>
          <div className="mb-1 text-xs uppercase tracking-wider text-zinc-500">{key}</div>
          <pre className="overflow-auto rounded border border-zinc-800 bg-zinc-900 p-3 font-mono text-xs text-zinc-300">
            {JSON.stringify(value, null, 2)}
          </pre>
        </div>
      ))}
    </div>
  );
}
