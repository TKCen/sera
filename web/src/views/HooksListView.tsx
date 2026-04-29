import { useQuery } from '@tanstack/react-query';
import { getHooks } from '@/lib/api';

export function HooksListView() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['hooks'],
    queryFn: getHooks,
  });

  return (
    <div className="space-y-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Hooks</h1>
        <p className="text-sm text-zinc-500">
          Hook modules registered with the gateway.
        </p>
      </div>

      {isLoading ? <div className="text-sm text-zinc-500">Loading…</div> : null}

      {error ? (
        <div className="rounded border border-red-900 bg-red-950/40 p-3 text-sm text-red-300">
          {(error as Error).message}
        </div>
      ) : null}

      {data && data.hooks.length === 0 ? (
        <div className="text-sm text-zinc-500">No hooks registered.</div>
      ) : null}

      {data && data.hooks.length > 0 ? (
        <ul className="divide-y divide-zinc-800 rounded-lg border border-zinc-800">
          {data.hooks.map((hook) => (
            <li key={hook.name} className="p-4">
              <div className="flex items-baseline justify-between gap-3">
                <span className="font-medium">{hook.name}</span>
                <span className="text-xs text-zinc-500">v{hook.version}</span>
              </div>
              {hook.description ? (
                <p className="mt-1 text-sm text-zinc-400">{hook.description}</p>
              ) : null}
              <div className="mt-2 flex flex-wrap gap-1.5">
                {hook.supported_points.map((point) => (
                  <span
                    key={point}
                    className="rounded bg-zinc-800 px-1.5 py-0.5 text-xs text-zinc-300"
                  >
                    {point}
                  </span>
                ))}
              </div>
              {hook.author ? (
                <div className="mt-2 text-xs text-zinc-500">by {hook.author}</div>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
