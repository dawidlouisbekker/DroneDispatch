import createClient from 'openapi-fetch';
import { apiBase } from '@/config';
import type { paths as CatalogPaths } from './generated/catalog';
import type { paths as MerchantPaths } from './generated/merchant';
import type { paths as UserPaths } from './generated/user';
import type { Problem } from './types';

// The user and merchant APIs expect an OAuth access token for their `resource` (see config.ts). The app
// doesn't obtain one yet; attach it with `client.use({ onRequest })` once it does. Catalog reads are public.
export const userApi = createClient<UserPaths>({ baseUrl: apiBase.user, credentials: 'include' });
export const merchantApi = createClient<MerchantPaths>({ baseUrl: apiBase.merchant, credentials: 'include' });
export const catalogApi = createClient<CatalogPaths>({ baseUrl: apiBase.catalog });

/** The message to show for a failed call: the Problem's detail or title, else `fallback`. */
export function errorMessage(cause: unknown, fallback: string): string {
  if (cause instanceof Error) return cause.message;
  const problem = cause as Partial<Problem> | null | undefined;
  if (typeof problem?.title === 'string') return problem.detail ?? problem.title;
  return fallback;
}
