import { apiBase } from '@/config';

export type Session = {
  user: { sub: string; email: string };
  auth_time: string;
  amr: string[];
  acr: string;
  second_factor_pending: boolean;
  factors: {
    passkeys: number;
  };
};

export type AuthRequest = {
  request_id: string;
  client: { name: string; first_party: boolean };
  resource: string;
  scopes: string[];
  redirect_host: string;
  next: 'sign_in' | 'mfa' | 'mfa_enrollment' | 'consent' | 'complete';
  session?: Session;
};

export class AuthError extends Error {
  constructor(
    message: string,
    readonly reason?: string,
    readonly status?: number,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`${apiBase.auth}/v1${path}`, {
    ...init,
    credentials: 'include',
    headers: {
      Accept: 'application/json',
      ...(init.body ? { 'Content-Type': 'application/json' } : {}),
      ...init.headers,
    },
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as {
      title?: string;
      message?: string;
      reason?: string;
    };
    throw new AuthError(body.title ?? body.message ?? 'Authentication request failed', body.reason, response.status);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export const authApi = {
  signIn: (email: string, password: string) =>
    request<Session>('/sign-in/password', { method: 'POST', body: JSON.stringify({ email, password }) }),
  checkSignupEmail: (email: string) =>
    request<{ exists: boolean }>('/sign-up/check', { method: 'POST', body: JSON.stringify({ email }) }),
  startSignup: (email: string) =>
    request<{ challenge_id: string; expires_at: string }>('/sign-up/otp', {
      method: 'POST',
      body: JSON.stringify({ email }),
    }),
  resendSignupOtp: (challengeId: string, email: string) =>
    request<{ challenge_id: string; expires_at: string }>('/sign-up/otp/resend', {
      method: 'POST',
      body: JSON.stringify({ challenge_id: challengeId, email }),
    }),
  verifySignupOtp: (challengeId: string, email: string, otp: string) =>
    request<{ verified: boolean }>('/sign-up/otp/verify', {
      method: 'POST',
      body: JSON.stringify({ challenge_id: challengeId, email, otp }),
    }),
  finalizeSignup: (challengeId: string, email: string, password: string) =>
    request<Session>('/sign-up/finalize', {
      method: 'POST',
      body: JSON.stringify({ challenge_id: challengeId, email, password }),
    }),
  passkeyOptions: () => request<{ challenge_id: string; options: Record<string, unknown> }>('/passkeys/options', { method: 'POST', body: '{}' }),
  addPasskey: (challengeId: string, name: string, credential: Record<string, unknown>) =>
    request<Session>('/passkeys', {
      method: 'POST',
      body: JSON.stringify({ challenge_id: challengeId, name, credential }),
    }),
  secondFactorOptions: () =>
    request<{ challenge_id: string; options: Record<string, unknown> }>('/mfa/passkey/options', { method: 'POST', body: '{}' }),
  verifyPasskey: (challengeId: string, credential: Record<string, unknown>) =>
    request<Session>('/mfa/passkey/verify', {
      method: 'POST',
      body: JSON.stringify({ challenge_id: challengeId, credential }),
    }),
  session: () => request<Session>('/session'),
  request: (id: string) => request<AuthRequest>(`/requests/${encodeURIComponent(id)}`),
  complete: (id: string, consent = false) =>
    request<{ redirect_to: string }>(`/requests/${encodeURIComponent(id)}/complete`, {
      method: 'POST',
      body: JSON.stringify({ consent }),
    }),
  signOut: () => request<void>('/sign-out', { method: 'POST', body: '{}' }),
};