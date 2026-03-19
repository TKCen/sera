import { useEffect } from 'react';
import { useNavigate } from 'react-router';
import { useAuthInternal } from '@/contexts/AuthContext';
import { Spinner } from '@/components/ui/spinner';

export default function AuthCallbackPage() {
  const navigate = useNavigate();
  const { setToken, getPKCEVerifier, clearPKCEVerifier } = useAuthInternal();

  useEffect(() => {
    async function handleCallback() {
      const params = new URLSearchParams(window.location.search);
      const code = params.get('code');
      const error = params.get('error');

      if (error) {
        console.error('OIDC error:', error);
        void navigate('/login');
        return;
      }

      if (!code) {
        void navigate('/login');
        return;
      }

      const verifier = getPKCEVerifier();
      if (!verifier) {
        void navigate('/login');
        return;
      }

      try {
        const tokenEndpoint =
          (import.meta as { env: Record<string, string> }).env['VITE_OIDC_TOKEN_URL'] ??
          '/api/auth/token';

        const response = await fetch(tokenEndpoint, {
          method: 'POST',
          headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
          body: new URLSearchParams({
            grant_type: 'authorization_code',
            code,
            redirect_uri: `${window.location.origin}/auth/callback`,
            code_verifier: verifier,
            client_id: (import.meta as { env: Record<string, string> }).env['VITE_OIDC_CLIENT_ID'] ?? 'sera-web',
          }),
        });

        if (!response.ok) throw new Error('Token exchange failed');

        const data = (await response.json()) as { access_token: string };
        clearPKCEVerifier();
        setToken(data.access_token);
        void navigate('/chat', { replace: true });
      } catch (err) {
        console.error('Auth callback error:', err);
        void navigate('/login');
      }
    }

    void handleCallback();
  }, [navigate, setToken, getPKCEVerifier, clearPKCEVerifier]);

  return (
    <div className="min-h-screen bg-sera-bg flex items-center justify-center">
      <div className="flex flex-col items-center gap-4">
        <Spinner size="lg" />
        <p className="text-sm text-sera-text-muted">Completing sign in…</p>
      </div>
    </div>
  );
}
