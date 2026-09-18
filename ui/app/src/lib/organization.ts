import type { Business } from '@/api/types';

/** Storage key for the business the user switched to in Settings → Account. */
export const ACTIVE_ORGANIZATION_KEY = 'dronedrop.active-organization';

/** The business with `businessId`, or null when there is no id or the user isn't a member. */
export function findBusiness(businesses: readonly Business[], businessId: string | null): Business | null {
  if (!businessId) return null;
  return businesses.find((business) => business.business_id === businessId) ?? null;
}
