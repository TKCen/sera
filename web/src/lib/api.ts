const API_BASE = '/api';
const AUTH_KEY = 'sera.auth.token';

export function getToken(): string | null {
  return localStorage.getItem(AUTH_KEY);
}

export function setToken(token: string | null): void {
  if (token) {
    localStorage.setItem(AUTH_KEY, token);
  } else {
    localStorage.removeItem(AUTH_KEY);
  }
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public body: unknown,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

export async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  const token = getToken();
  if (token) headers.set('Authorization', `Bearer ${token}`);
  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  const res = await fetch(`${API_BASE}${path}`, { ...init, headers });
  const text = await res.text();
  const body = text ? safeJson(text) : null;

  if (!res.ok) {
    const msg =
      typeof body === 'object' && body && 'error' in body
        ? String((body as { error: unknown }).error)
        : res.statusText;
    throw new ApiError(res.status, body, msg);
  }

  return body as T;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

// ── Typed endpoints ────────────────────────────────────────────────────────

export interface Health {
  status: 'ok';
}

export interface Readiness {
  status: 'ready' | 'not_ready';
  runtime_connected: boolean;
}

export interface AuthMe {
  id: string;
  principal_id: string;
  sub: string;
  roles: string[];
  mode: 'autonomous' | string;
}

export const getHealth = () => apiFetch<Health>('/health');

export const getReadiness = () =>
  apiFetch<Readiness>('/health/ready').catch((err) => {
    // 503 is a valid readiness response carrying {status:"not_ready", ...}
    if (err instanceof ApiError && err.status === 503 && err.body) {
      return err.body as Readiness;
    }
    throw err;
  });

export const getAuthMe = () => apiFetch<AuthMe>('/auth/me');
