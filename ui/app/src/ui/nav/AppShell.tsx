import { useState } from 'react';
import { Slot } from 'expo-router';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { Button, Text, XStack, YStack } from 'tamagui';
import { useOrganization } from '@/session/OrganizationProvider';
import { AppDrawer } from './AppDrawer';

/** Signed-in frame: a top bar with the menu button, the current screen, and the drawer over both. */
export function AppShell() {
  const [menuOpen, setMenuOpen] = useState(false);
  const { activeBusiness } = useOrganization();
  const insets = useSafeAreaInsets();

  return (
    <YStack flex={1} bg="$background">
      <XStack
        items="center"
        gap="$2"
        px="$2"
        pt={insets.top + 8}
        pb="$2"
        borderBottomWidth={1}
        borderColor="$borderColor"
      >
        <Button chromeless circular aria-label="Open menu" onPress={() => setMenuOpen(true)}>
          <YStack gap={4}>
            <YStack width={18} height={2} rounded={1} bg="$color12" />
            <YStack width={18} height={2} rounded={1} bg="$color12" />
            <YStack width={18} height={2} rounded={1} bg="$color12" />
          </YStack>
        </Button>
        <Text fontSize="$5" fontWeight="700" color="$color12">Drone Drop</Text>
        {activeBusiness ? (
          <Text color="$color11" numberOfLines={1} shrink={1}>· {activeBusiness.name}</Text>
        ) : null}
      </XStack>
      <YStack flex={1}>
        <Slot />
      </YStack>
      <AppDrawer open={menuOpen} onClose={() => setMenuOpen(false)} />
    </YStack>
  );
}
