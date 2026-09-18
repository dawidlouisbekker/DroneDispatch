import { Redirect, Slot } from 'expo-router';
import { Spinner, YStack } from 'tamagui';
import { useOrganization } from '@/session/OrganizationProvider';

/** Organization screens need an active organization; without one, send the user to pick it. */
export default function OrganizationLayout() {
  const { activeBusiness, status } = useOrganization();

  if (status === 'loading') {
    return (
      <YStack flex={1} items="center" justify="center" bg="$background">
        <Spinner color="$blue10" />
      </YStack>
    );
  }
  if (!activeBusiness) return <Redirect href="/settings/account" />;
  return <Slot />;
}
