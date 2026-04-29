import { useQuery } from '@tanstack/react-query';
import { Link, useParams } from 'react-router';
import { ApiError, getSessionTranscript, type TranscriptEntry } from '@/lib/api';
import { cn } from '@/lib/utils';

export function SessionDetailView() {
  const { id } = useParams<{ id: string }>();
  const sessionId = id ? decodeURIComponent(id) : '';

  const { isLoading, error, data } = useQuery({
    queryKey: ['session-transcript', sessionId],
    queryFn: () => getSessionTranscript(sessionId),
    enabled: Boolean(sessionId),
  });

  const isNotFound = error instanceof ApiError && error.status === 404;

  return (
    <div className="space-y-6 p-6">
      <div className="flex items-center gap-4">
        <Link to="/sessions" className="text-sm text-zinc-400 hover:text-zinc-200">
          ← Back to sessions
        </Link>
      </div>

      <div>
        <h1 className="font-mono text-xl font-semibold">{sessionId}</h1>
        <p className="text-sm text-zinc-500">Session transcript</p>
      </div>

      {isLoading && <p className="text-sm text-zinc-400">Loading…</p>}

      {isNotFound && (
        <p className="text-sm text-zinc-500">Session not found.</p>
      )}

      {error && !isNotFound && (
        <p className="text-sm text-red-400">
          Failed to load transcript: {(error as Error).message}
        </p>
      )}

      {!isLoading && !error && data?.length === 0 && (
        <p className="text-sm text-zinc-500">No transcript entries.</p>
      )}

      {data && data.length > 0 && (
        <div className="space-y-3">
          {data.map((entry: TranscriptEntry) => (
            <TranscriptRow key={entry.id} entry={entry} />
          ))}
        </div>
      )}
    </div>
  );
}

function TranscriptRow({ entry }: { entry: TranscriptEntry }) {
  const role = entry.role;

  if (role === 'user' || role === 'assistant' || role === 'system' || role === 'tool') {
    return (
      <div
        className={cn(
          'rounded-lg border p-3',
          role === 'user' && 'border-blue-800 bg-blue-950/30',
          role === 'assistant' && 'border-emerald-800 bg-emerald-950/30',
          role === 'system' && 'border-zinc-700 bg-zinc-900',
          role === 'tool' && 'border-amber-800 bg-amber-950/30',
        )}
      >
        <div className="mb-1 flex items-center justify-between">
          <span
            className={cn(
              'text-xs font-semibold uppercase tracking-wider',
              role === 'user' && 'text-blue-400',
              role === 'assistant' && 'text-emerald-400',
              role === 'system' && 'text-zinc-400',
              role === 'tool' && 'text-amber-400',
            )}
          >
            {role}
            {entry.tool_call_id ? ` · ${entry.tool_call_id}` : ''}
          </span>
          <span className="text-xs text-zinc-600">{entry.created_at}</span>
        </div>
        {entry.content && (
          <p className="whitespace-pre-wrap text-sm text-zinc-200">{entry.content}</p>
        )}
        {entry.tool_calls && (
          <pre className="mt-1 overflow-x-auto rounded bg-zinc-800 p-2 text-xs text-zinc-300">
            {entry.tool_calls}
          </pre>
        )}
      </div>
    );
  }

  // Unknown role — fall back to raw JSON block
  return (
    <div className="rounded-lg border border-zinc-700 p-3">
      <div className="mb-1 text-xs font-semibold uppercase tracking-wider text-zinc-500">
        {role}
      </div>
      <pre className="overflow-x-auto rounded bg-zinc-800 p-2 text-xs text-zinc-300">
        {JSON.stringify(entry, null, 2)}
      </pre>
    </div>
  );
}
