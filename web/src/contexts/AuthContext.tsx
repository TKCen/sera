/// <reference types="vite/client" />
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react';
import { setAuthHeaderGetter, onUnauthorized } from '@/lib/api/client';

interface User {
  sub: string;
  name?: string;
  email?: string;
  roles?: string[];
}

interface AuthContextValue {
  isAuthenticated: boolean;
  user: User | null;
  roles: string[];
  isLoading: boolean;
  login: () => void;
  logout: () => void;
}

const AuthContext = createContext<AuthContextValue>({
  isAuthenticated: false,
  user: null,
  roles: [],
  isLoading: true,
  login: () => undefined,
  logout: () => undefined,
});

// Keys for temporary PKCE state (cleared immediately after callback)
const SESSION_KEY_SESSION_TOKEN = 'sera_session_token';
const SESSION_KEY_USER = 'sera_user';
const SESSION_KEY_PKCE_VERIFIER = 'sera_pkce_verifier';

const DEV_KEY = import.meta.env.VITE_DEV_API_KEY;

function generatePKCEVerifier(): string {
  const array = new Uint8Array(32);
  crypto.getRandomValues(array);
  return btoa(String.fromCharCode(...array))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=/g, '');
}

async function generatePKCEChallenge(verifier: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(verifier);
  const digest = await crypto.subtle.digest('SHA-256', data);
  return btoa(String.fromCharCode(...new Uint8Array(digest)))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=/g, '');
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [sessionToken, setSessionTokenState] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const isDevMode = Boolean(DEV_KEY);

  const logout = useCallback(async () => {
    const token = sessionStorage.getItem(SESSION_KEY_SESSION_TOKEN);
    if (token) {
      // Best-effort revocation
      await fetch('/api/auth/logout', {
        method: 'POST',
        headers: { Authorization: `Bearer ${token}` },
      }).catch(() => {});
    }
    sessionStorage.removeItem(SESSION_KEY_SESSION_TOKEN);
    sessionStorage.removeItem(SESSION_KEY_USER);
    setSessionTokenState(null);
    setUser(null);
    if (!isDevMode) {
      window.location.href = '/login';
    }
  }, [isDevMode]);

  const login = useCallback(async () => {
    if (isDevMode) return;

    const verifier = generatePKCEVerifier();
    const challenge = await generatePKCEChallenge(verifier);
    sessionStorage.setItem(SESSION_KEY_PKCE_VERIFIER, verifier);

    const params = new URLSearchParams({
      response_type: 'code',
      client_id: import.meta.env.VITE_OIDC_CLIENT_ID ?? 'sera-web',
      redirect_uri: `${window.location.origin}/auth/callback`,
      scope: 'openid profile email',
      code_challenge: challenge,
      code_challenge_method: 'S256',
    });

    const authEndpoint = import.meta.env.VITE_OIDC_AUTHORIZE_URL ?? '/api/auth/login';
    if (authEndpoint === '/api/auth/login') {
      // Let sera-core redirect to IdP
      window.location.href = authEndpoint;
    } else {
      window.location.href = `${authEndpoint}?${params.toString()}`;
    }
  }, [isDevMode]);

  useEffect(() => {
    if (isDevMode) {
      setUser({ sub: 'dev', name: 'Developer', roles: ['admin'] });
      setIsLoading(false);
      return;
    }

    // Restore session from sessionStorage (session token is opaque — not the OIDC access token)
    const storedToken = sessionStorage.getItem(SESSION_KEY_SESSION_TOKEN);
    const storedUser = sessionStorage.getItem(SESSION_KEY_USER);
    if (storedToken && storedUser) {
      try {
        const parsed = JSON.parse(storedUser) as User;
        setSessionTokenState(storedToken);
        setUser(parsed);
      } catch {
        sessionStorage.removeItem(SESSION_KEY_SESSION_TOKEN);
        sessionStorage.removeItem(SESSION_KEY_USER);
      }
    }
    setIsLoading(false);
  }, [isDevMode]);

  // Wire auth header getter
  useEffect(() => {
    setAuthHeaderGetter(() => {
      if (isDevMode && DEV_KEY) return `Bearer ${DEV_KEY}`;
      const t = sessionStorage.getItem(SESSION_KEY_SESSION_TOKEN);
      return t ? `Bearer ${t}` : null;
    });
  }, [isDevMode]);

  // Handle 401 from API
  useEffect(() => {
    return onUnauthorized(logout);
  }, [logout]);

  const setSessionAndUser = useCallback((token: string, userData: User) => {
    sessionStorage.setItem(SESSION_KEY_SESSION_TOKEN, token);
    sessionStorage.setItem(SESSION_KEY_USER, JSON.stringify(userData));
    setSessionTokenState(token);
    setUser(userData);
  }, []);

  const isAuthenticated = isDevMode || sessionToken !== null;

  return (
    <AuthContext.Provider
      value={{
        isAuthenticated,
        user,
        roles: user?.roles ?? [],
        isLoading,
        login: () => {
          void login();
        },
        logout,
      }}
    >
      <AuthContextInternal setSessionAndUser={setSessionAndUser}>{children}</AuthContextInternal>
    </AuthContext.Provider>
  );
}

// Internal context for callback page to wire session after OIDC exchange
interface AuthInternalContextValue {
  setSessionAndUser: (token: string, user: User) => void;
  getPKCEVerifier: () => string | null;
  clearPKCEVerifier: () => void;
}

const AuthInternalContext = createContext<AuthInternalContextValue>({
  setSessionAndUser: () => undefined,
  getPKCEVerifier: () => null,
  clearPKCEVerifier: () => undefined,
});

function AuthContextInternal({
  children,
  setSessionAndUser,
}: {
  children: ReactNode;
  setSessionAndUser: (token: string, user: User) => void;
}) {
  return (
    <AuthInternalContext.Provider
      value={{
        setSessionAndUser,
        getPKCEVerifier: () => sessionStorage.getItem(SESSION_KEY_PKCE_VERIFIER),
        clearPKCEVerifier: () => sessionStorage.removeItem(SESSION_KEY_PKCE_VERIFIER),
      }}
    >
      {children}
    </AuthInternalContext.Provider>
  );
}

export function useAuth(): AuthContextValue {
  return useContext(AuthContext);
}

export function useAuthInternal(): AuthInternalContextValue {
  return useContext(AuthInternalContext);
}
