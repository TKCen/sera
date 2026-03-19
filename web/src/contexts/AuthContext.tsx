/// <reference types="vite/client" />
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from 'react';
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
  token: string | null;
  isLoading: boolean;
  login: () => void;
  logout: () => void;
}

const AuthContext = createContext<AuthContextValue>({
  isAuthenticated: false,
  user: null,
  roles: [],
  token: null,
  isLoading: true,
  login: () => undefined,
  logout: () => undefined,
});

const SESSION_KEY_TOKEN = 'sera_access_token';
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

function parseToken(token: string): User | null {
  try {
    const payload = token.split('.')[1];
    if (!payload) return null;
    const decoded = JSON.parse(atob(payload.replace(/-/g, '+').replace(/_/g, '/'))) as Record<string, unknown>;
    return {
      sub: decoded['sub'] as string,
      name: decoded['name'] as string | undefined,
      email: decoded['email'] as string | undefined,
      roles: (decoded['roles'] ?? decoded['groups'] ?? []) as string[],
    };
  } catch {
    return null;
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setToken] = useState<string | null>(null);
  const [user, setUser] = useState<User | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // Dev mode: API key bypass
  const isDevMode = Boolean(DEV_KEY);

  const logout = useCallback(() => {
    sessionStorage.removeItem(SESSION_KEY_TOKEN);
    sessionStorage.removeItem(SESSION_KEY_USER);
    setToken(null);
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

    const authEndpoint = import.meta.env.VITE_OIDC_AUTHORIZE_URL ?? '/api/auth/authorize';
    window.location.href = `${authEndpoint}?${params.toString()}`;
  }, [isDevMode]);

  useEffect(() => {
    if (isDevMode) {
      setToken(DEV_KEY);
      setUser({ sub: 'dev', name: 'Developer', roles: ['operator'] });
      setIsLoading(false);
      return;
    }

    // Restore from sessionStorage
    const stored = sessionStorage.getItem(SESSION_KEY_TOKEN);
    if (stored) {
      const parsed = parseToken(stored);
      if (parsed) {
        setToken(stored);
        setUser(parsed);
      } else {
        sessionStorage.removeItem(SESSION_KEY_TOKEN);
      }
    }
    setIsLoading(false);
  }, [isDevMode]);

  // Wire auth header getter to client
  useEffect(() => {
    setAuthHeaderGetter(() => {
      if (isDevMode && DEV_KEY) return `Bearer ${DEV_KEY}`;
      const t = sessionStorage.getItem(SESSION_KEY_TOKEN);
      return t ? `Bearer ${t}` : null;
    });
  }, [isDevMode]);

  // Handle 401 from API
  useEffect(() => {
    return onUnauthorized(logout);
  }, [logout]);

  const setTokenAndPersist = useCallback((newToken: string) => {
    sessionStorage.setItem(SESSION_KEY_TOKEN, newToken);
    const parsed = parseToken(newToken);
    setToken(newToken);
    setUser(parsed);
  }, []);

  const isAuthenticated = isDevMode || token !== null;

  return (
    <AuthContext.Provider
      value={{
        isAuthenticated,
        user,
        roles: user?.roles ?? [],
        token,
        isLoading,
        login: () => { void login(); },
        logout,
      }}
    >
      <AuthContextInternal setToken={setTokenAndPersist}>
        {children}
      </AuthContextInternal>
    </AuthContext.Provider>
  );
}

// Internal context for token setter (used by AuthCallbackPage)
interface AuthInternalContextValue {
  setToken: (token: string) => void;
  getPKCEVerifier: () => string | null;
  clearPKCEVerifier: () => void;
}

const AuthInternalContext = createContext<AuthInternalContextValue>({
  setToken: () => undefined,
  getPKCEVerifier: () => null,
  clearPKCEVerifier: () => undefined,
});

function AuthContextInternal({
  children,
  setToken,
}: {
  children: ReactNode;
  setToken: (token: string) => void;
}) {
  return (
    <AuthInternalContext.Provider
      value={{
        setToken,
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
