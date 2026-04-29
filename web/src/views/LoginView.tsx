import { useState, type FormEvent } from 'react';
import { Navigate, useLocation } from 'react-router';
import { ApiError } from '@/lib/api';
import { useAuth } from '@/contexts/auth-context';

export function LoginView() {
  const { status, signIn } = useAuth();
  const location = useLocation();
  const [token, setTokenInput] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (status === 'authenticated') {
    const from = (location.state as { from?: string } | null)?.from ?? '/';
    return <Navigate to={from} replace />;
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!token.trim() || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await signIn(token.trim());
    } catch (err) {
      const msg =
        err instanceof ApiError && err.status === 401
          ? 'API key rejected by the gateway.'
          : err instanceof Error
            ? err.message
            : 'Sign-in failed.';
      setError(msg);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="flex h-full items-center justify-center bg-zinc-950 p-6">
      <form
        onSubmit={onSubmit}
        className="w-full max-w-sm space-y-4 rounded-lg border border-zinc-800 bg-zinc-900 p-6"
      >
        <div>
          <h1 className="text-lg font-semibold">SERA</h1>
          <p className="text-xs text-zinc-500">Sign in with an API key.</p>
        </div>
        <label className="block text-xs text-zinc-400">
          API key
          <input
            type="password"
            autoComplete="off"
            autoFocus
            value={token}
            onChange={(e) => setTokenInput(e.target.value)}
            className="mt-1 block w-full rounded border border-zinc-700 bg-zinc-950 px-2 py-1.5 text-sm text-zinc-100 outline-none focus:border-zinc-500"
            placeholder="sera_…"
          />
        </label>
        {error ? <p className="text-xs text-red-400">{error}</p> : null}
        <button
          type="submit"
          disabled={!token.trim() || submitting}
          className="w-full rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {submitting ? 'Signing in…' : 'Sign in'}
        </button>
      </form>
    </div>
  );
}
