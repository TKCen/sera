/// <reference types="vite/client" />
import { useState } from 'react';
import { Navigate } from 'react-router';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { CardGlass, CardHeader, CardTitle, CardDescription, CardContent } from '@/components/ui/card';
import { useAuth } from '@/contexts/AuthContext';

export default function LoginPage() {
  const { isAuthenticated, login } = useAuth();
  const [apiKey, setApiKey] = useState('');
  const isDevMode = Boolean(import.meta.env.VITE_DEV_API_KEY);

  if (isAuthenticated) {
    return <Navigate to="/chat" replace />;
  }

  const handleDevLogin = (e: React.FormEvent) => {
    e.preventDefault();
    // Store the API key in sessionStorage for manual dev key entry
    if (apiKey) {
      sessionStorage.setItem('sera_access_token', apiKey);
      window.location.href = '/chat';
    }
  };

  return (
    <div className="min-h-screen bg-sera-bg flex items-center justify-center p-4">
      <div className="w-full max-w-md">
        <div className="text-center mb-8">
          <div className="inline-flex h-16 w-16 items-center justify-center rounded-2xl bg-sera-accent mb-4">
            <span className="text-sera-bg text-2xl font-bold">S</span>
          </div>
          <h1 className="text-2xl font-semibold text-sera-text">Welcome to SERA</h1>
          <p className="text-sm text-sera-text-muted mt-1">Sandboxed Extensible Reasoning Agent</p>
        </div>

        <CardGlass className="p-6">
          <CardHeader>
            <CardTitle>Sign In</CardTitle>
            <CardDescription>
              {isDevMode
                ? 'Running in dev mode with API key bypass.'
                : 'Sign in to access your agent platform.'}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {isDevMode ? (
              <div className="text-center py-4">
                <p className="text-sm text-sera-success mb-4">
                  ✓ Dev API key configured via VITE_DEV_API_KEY
                </p>
                <Button onClick={login} className="w-full">
                  Continue to Dashboard
                </Button>
              </div>
            ) : (
              <div className="space-y-4">
                <Button onClick={login} className="w-full" size="lg">
                  Sign in with SSO
                </Button>

                <div className="relative">
                  <div className="absolute inset-0 flex items-center">
                    <div className="w-full border-t border-sera-border" />
                  </div>
                  <div className="relative flex justify-center text-xs">
                    <span className="bg-sera-bg px-2 text-sera-text-muted">or</span>
                  </div>
                </div>

                <form onSubmit={handleDevLogin} className="space-y-3">
                  <Input
                    type="password"
                    placeholder="API Key (development only)"
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    aria-label="API Key"
                  />
                  <Button type="submit" variant="secondary" className="w-full" disabled={!apiKey}>
                    Continue with API Key
                  </Button>
                </form>
              </div>
            )}
          </CardContent>
        </CardGlass>
      </div>
    </div>
  );
}
