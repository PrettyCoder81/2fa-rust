const BASE = import.meta.env.VITE_API_URL ?? 'http://172.20.5.123:8000';

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method,
    credentials: 'include',
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  if (res.status === 204) return {} as T;

  if (!res.ok) {
    let msg = `Request failed (${res.status})`;
    try {
      const data = await res.json();
      msg = data.message || data.error || msg;
    } catch {}
    throw new Error(msg);
  }

  return res.json();
}

export interface ActiveSession {
  id: string;
  device_name?: string;
  ip_address?: string;
  user_agent?: string;
  created_at: string;
  last_used_at: string;
  expires_at: string;
  current: boolean;
}

export interface Profile {
  id: string;
  email: string;
  two_factor_enabled: boolean;
}

export const api = {
  profile: () => request<Profile>('GET', '/api/profile'),

  register: (data: { username: string; email: string; password: string }) =>
    request<{ id: string; email: string }>('POST', '/api/register', data),

  login: (data: { email: string; password: string }) =>
    request<{ message: string; requires_2fa: boolean; challenge_token?: string }>(
      'POST', '/api/login', data
    ),

  logout: () => request<void>('POST', '/api/logout'),
  logoutAll: () => request<void>('POST', '/api/logout-all'),

  getSessions: () => request<ActiveSession[]>('GET', '/api/sessions'),
  revokeSession: (id: string) => request<void>('DELETE', `/api/sessions/${id}`),

  setup2fa: () => request<{ secret: string; otpauth_url: string }>('POST', '/api/2fa/setup'),
  confirm2fa: (data: { code: string }) =>
    request<{ message: string; recovery_codes: string[] }>('POST', '/api/2fa/confirm', data),
  verify2fa: (data: { challenge_token: string; code: string }) =>
    request<{ message: string }>('POST', '/api/2fa/verify', data),
  disable2fa: (data: { password: string; code: string }) =>
    request<void>('POST', '/api/2fa/disable', data),
  verifyRecovery: (data: { challenge_token: string; code: string }) =>
    request<void>('POST', '/api/2fa/recovery', data),
  regenerateRecoveryCodes: (data: { password: string; code: string }) =>
    request<{ recovery_codes: string[] }>('POST', '/api/2fa/recovery-codes/regenerate', data),

  changePassword: (data: { current_password: string; new_password: string }) =>
    request<void>('POST', '/api/password/change', data),
};
