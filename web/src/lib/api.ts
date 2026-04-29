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
  if (shouldAttachJsonContentType(init.body, headers)) {
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

function shouldAttachJsonContentType(body: BodyInit | null | undefined, headers: Headers): boolean {
  if (!body || headers.has('Content-Type')) return false;
  // Browser-managed bodies set their own Content-Type (with multipart boundary etc.).
  // Only auto-set application/json for plain string payloads.
  return typeof body === 'string';
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
  mode: string;
}

export const getHealth = () => apiFetch<Health>('/health');

export const getReadiness = () =>
  apiFetch<Readiness>('/health/ready').catch((err) => {
    // 503 carries the same {status, runtime_connected} shape on the not-ready path.
    if (err instanceof ApiError && err.status === 503 && isReadiness(err.body)) {
      return err.body;
    }
    throw err;
  });

function isReadiness(value: unknown): value is Readiness {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    (v.status === 'ready' || v.status === 'not_ready') &&
    typeof v.runtime_connected === 'boolean'
  );
}

export const getAuthMe = () => apiFetch<AuthMe>('/auth/me');
