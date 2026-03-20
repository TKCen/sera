import { useEffect } from 'react';
import { useNavigate } from 'react-router';
import { useAuthInternal } from '@/hooks/useAuth';
import { Spinner } from '@/components/ui/spinner';

interface OIDCCallbackResponse {
  sessionToken: string;
  user: {
    sub: string;
    name?: string;
    email?: string;
    roles?: string[];
  };
}

export default function AuthCallbackPage() {
  const navigate = useNavigate();
  const { setSessionAndUser, getPKCEVerifier, clearPKCEVerifier } = useAuthInternal();

  useEffect(() => {
    async function handleCallback() {
      const params = new URLSearchParams(window.location.search);
      const code = params.get('code');
      const error = params.get('error');

      if (error) {
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
        // Send code + PKCE verifier to sera-core for server-side token exchange.
        // The OIDC access token never leaves the server — we receive an opaque
        // session token and user identity instead.
        const response = await fetch('/api/auth/oidc/callback', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            code,
            codeVerifier: verifier,
            redirectUri: `${window.location.origin}/auth/callback`,
            clientId: import.meta.env.VITE_OIDC_CLIENT_ID ?? 'sera-web',
          }),
        });

        if (!response.ok) throw new Error('Callback exchange failed');

        const data = (await response.json()) as OIDCCallbackResponse;
        clearPKCEVerifier();
        setSessionAndUser(data.sessionToken, data.user);
        void navigate('/chat', { replace: true });
      } catch {
        void navigate('/login');
      }
    }

    void handleCallback();
  }, [navigate, setSessionAndUser, getPKCEVerifier, clearPKCEVerifier]);

  return (
    <div className="min-h-screen bg-sera-bg flex items-center justify-center">
      <div className="flex flex-col items-center gap-4">
        <Spinner size="lg" />
        <p className="text-sm text-sera-text-muted">Completing sign in…</p>
      </div>
    </div>
  );
}
