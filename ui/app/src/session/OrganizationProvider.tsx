import { createContext, useCallback, useContext, useEffect, useState } from 'react';
import { errorMessage, merchantApi } from '@/api/clients';
import type { Business } from '@/api/types';
import { ACTIVE_ORGANIZATION_KEY, findBusiness } from '@/lib/organization';
import { storage } from '@/lib/storage';

type OrganizationContextValue = {
  /** Businesses the signed-in user belongs to. */
  businesses: Business[];
  status: 'loading' | 'ready' | 'error';
  error: string;
  /** The business the user switched to, or null in personal mode. */
  activeBusiness: Business | null;
  switchTo: (businessId: string) => Promise<void>;
  leave: () => Promise<void>;
};

const OrganizationContext = createContext<OrganizationContextValue | null>(null);

export function OrganizationProvider({ children }: { children: React.ReactNode }) {
  const [businesses, setBusinesses] = useState<Business[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [status, setStatus] = useState<OrganizationContextValue['status']>('loading');
  const [error, setError] = useState('');

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      const storedId = await storage.get(ACTIVE_ORGANIZATION_KEY).catch(() => null);
      try {
        const { data, error: problem } = await merchantApi.GET('/v1/businesses');
        if (!data) throw problem;
        if (cancelled) return;
        setBusinesses(data.businesses);
        setStatus('ready');
      } catch (cause) {
        if (cancelled) return;
        setError(errorMessage(cause, 'Could not load your organizations'));
        setStatus('error');
      }
      setActiveId(storedId);
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  const switchTo = useCallback(async (businessId: string) => {
    setActiveId(businessId);
    await storage.set(ACTIVE_ORGANIZATION_KEY, businessId);
  }, []);

  const leave = useCallback(async () => {
    setActiveId(null);
    await storage.remove(ACTIVE_ORGANIZATION_KEY);
  }, []);

  const activeBusiness = findBusiness(businesses, activeId);

  return (
    <OrganizationContext value={{ businesses, status, error, activeBusiness, switchTo, leave }}>
      {children}
    </OrganizationContext>
  );
}

export function useOrganization(): OrganizationContextValue {
  const value = useContext(OrganizationContext);
  if (!value) throw new Error('useOrganization must be used inside OrganizationProvider');
  return value;
}
