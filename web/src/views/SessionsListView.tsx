import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router';
import { getSessions, type SessionInfo } from '@/lib/api';

export function SessionsListView() {
  const { isLoading, error, data } = useQuery({
    queryKey: ['sessions'],
    queryFn: getSessions,
  });

  return (
    <div className="space-y-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Sessions</h1>
        <p className="text-sm text-zinc-500">All agent sessions recorded by the gateway.</p>
      </div>

      {isLoading && <p className="text-sm text-zinc-400">Loading…</p>}

      {error && (
        <p className="text-sm text-red-400">
          Failed to load sessions: {(error as Error).message}
        </p>
      )}

      {!isLoading && !error && data?.length === 0 && (
        <p className="text-sm text-zinc-500">No sessions found.</p>
      )}

      {data && data.length > 0 && (
        <ul className="divide-y divide-zinc-800 rounded-lg border border-zinc-800">
          {data.map((session: SessionInfo) => (
            <li key={session.id}>
              <Link
                to={`/sessions/${encodeURIComponent(session.id)}`}
                className="flex items-center justify-between px-4 py-3 hover:bg-zinc-900"
              >
                <div className="min-w-0">
                  <div className="truncate font-mono text-sm text-zinc-100">{session.id}</div>
                  <div className="mt-0.5 text-xs text-zinc-500">
                    agent: {session.agent_id} · state: {session.state}
                    {session.principal_id ? ` · principal: ${session.principal_id}` : ''}
                  </div>
                </div>
                <div className="ml-4 shrink-0 text-right">
                  <div className="text-xs text-zinc-400">{session.created_at}</div>
                  {session.updated_at && (
                    <div className="text-xs text-zinc-600">upd: {session.updated_at}</div>
                  )}
                </div>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
