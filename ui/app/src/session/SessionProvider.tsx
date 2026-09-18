import { createContext, useCallback, useContext, useState } from 'react';
import { authApi, Session } from '@/api/auth';
import { ACTIVE_ORGANIZATION_KEY } from '@/lib/organization';
import { storage } from '@/lib/storage';

type SessionContextValue = {
  session: Session | null;
  /** True until the first session check finishes. */
  loading: boolean;
  /** Re-reads the session from auth-service. The (app) layout calls this whenever it gains focus. */
  refresh: () => Promise<void>;
  signOut: () => Promise<void>;
};

const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({ children }: { children: React.ReactNode }) {
  const [session, setSession] = useState<Session | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    setSession(await authApi.session().catch(() => null));
    setLoading(false);
  }, []);

  const signOut = useCallback(async () => {
    await authApi.signOut();
    await storage.remove(ACTIVE_ORGANIZATION_KEY);
    setSession(null);
  }, []);

  return <SessionContext value={{ session, loading, refresh, signOut }}>{children}</SessionContext>;
}

export function useSession(): SessionContextValue {
  const value = useContext(SessionContext);
  if (!value) throw new Error('useSession must be used inside SessionProvider');
  return value;
}
