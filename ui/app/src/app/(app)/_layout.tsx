import { useCallback } from 'react';
import { Redirect, useFocusEffect, usePathname } from 'expo-router';
import { Spinner, YStack } from 'tamagui';
import { OrganizationProvider } from '@/session/OrganizationProvider';
import { useSession } from '@/session/SessionProvider';
import { Landing } from '@/ui/Landing';
import { AppShell } from '@/ui/nav/AppShell';

/** Gate for the signed-in app: landing page or sign-in for visitors, the drawer shell for members. */
export default function AppLayout() {
  const { session, loading, refresh } = useSession();
  const pathname = usePathname();

  // Re-check the session whenever the app comes back into view, e.g. after sign-in or MFA.
  useFocusEffect(
    useCallback(() => {
      void refresh();
    }, [refresh]),
  );

  if (loading) {
    return (
      <YStack flex={1} items="center" justify="center" bg="$background">
        <Spinner color="$blue10" />
      </YStack>
    );
  }
  if (!session) return pathname === '/' ? <Landing /> : <Redirect href="/sign-in" />;
  if (session.second_factor_pending) return <Redirect href="/mfa" />;

  return (
    <OrganizationProvider>
      <AppShell />
    </OrganizationProvider>
  );
}
