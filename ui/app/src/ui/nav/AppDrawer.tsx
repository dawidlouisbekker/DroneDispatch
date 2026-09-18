import { useEffect, useState } from 'react';
import { BackHandler, Platform, Pressable, StyleSheet, useWindowDimensions, View } from 'react-native';
import Animated, { useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { router, usePathname } from 'expo-router';
import { Accordion, Button, Paragraph, Separator, Text, YStack } from 'tamagui';
import { ORGANIZATION_LINKS, SETTINGS_LINKS, sectionsForPath } from '@/lib/nav';
import { useOrganization } from '@/session/OrganizationProvider';
import { useSession } from '@/session/SessionProvider';
import { NavItem } from './NavItem';
import { NavSection } from './NavSection';

const DURATION_MS = 220;
const MAX_WIDTH = 280;
const BACKDROP_OPACITY = 0.45;

type AppDrawerProps = { open: boolean; onClose: () => void };

/** Navigation panel that slides in from the left over a dimmed backdrop. */
export function AppDrawer({ open, onClose }: AppDrawerProps) {
  const [mounted, setMounted] = useState(open);
  const progress = useSharedValue(0);
  const { width } = useWindowDimensions();
  const panelWidth = Math.min(MAX_WIDTH, width * 0.85);
  const insets = useSafeAreaInsets();

  useEffect(() => {
    progress.set(withTiming(open ? 1 : 0, { duration: DURATION_MS }));
    if (open) return;
    // Keep the panel rendered until it has slid out.
    const timer = setTimeout(() => setMounted(false), DURATION_MS);
    return () => clearTimeout(timer);
  }, [open, progress]);

  useEffect(() => {
    if (!open) return;
    if (Platform.OS === 'web') {
      const onKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') onClose();
      };
      document.addEventListener('keydown', onKeyDown);
      return () => document.removeEventListener('keydown', onKeyDown);
    }
    const subscription = BackHandler.addEventListener('hardwareBackPress', () => {
      onClose();
      return true;
    });
    return () => subscription.remove();
  }, [open, onClose]);

  const backdropStyle = useAnimatedStyle(() => ({ opacity: progress.get() * BACKDROP_OPACITY }));
  const panelStyle = useAnimatedStyle(() => ({ transform: [{ translateX: (progress.get() - 1) * panelWidth }] }));

  if (open && !mounted) setMounted(true);
  if (!mounted) return null;

  return (
    <View style={[StyleSheet.absoluteFill, { pointerEvents: open ? 'auto' : 'none' }]}>
      <Animated.View style={[StyleSheet.absoluteFill, styles.backdrop, backdropStyle]}>
        <Pressable aria-label="Close menu" style={StyleSheet.absoluteFill} onPress={onClose} />
      </Animated.View>
      <Animated.View style={[styles.panel, { width: panelWidth }, panelStyle]}>
        <YStack
          flex={1}
          bg="$background"
          borderRightWidth={1}
          borderColor="$borderColor"
          pt={insets.top + 16}
          pb={insets.bottom + 16}
          px="$3"
          gap="$2"
        >
          <DrawerMenu onNavigate={onClose} />
        </YStack>
      </Animated.View>
    </View>
  );
}

function DrawerMenu({ onNavigate }: { onNavigate: () => void }) {
  const { session, signOut } = useSession();
  const { activeBusiness } = useOrganization();
  const pathname = usePathname();
  // The drawer unmounts when closed, so each opening starts with the current screen's section expanded.
  const [openSections, setOpenSections] = useState(() => sectionsForPath(pathname));

  return (
    <>
      <Text px="$4" pb="$2" fontSize="$6" fontWeight="700" color="$color12">Drone Drop</Text>
      <YStack flex={1} gap="$1">
        <NavItem label="Orders" href="/" onNavigate={onNavigate} />
        <Accordion type="multiple" value={openSections} onValueChange={setOpenSections}>
          <NavSection value="settings" label="Settings" open={openSections.includes('settings')}>
            {SETTINGS_LINKS.map((link) => <NavItem key={link.href} {...link} onNavigate={onNavigate} />)}
          </NavSection>
          {activeBusiness ? (
            <NavSection
              value="organization"
              label="Organization"
              subtitle={activeBusiness.name}
              open={openSections.includes('organization')}
            >
              {ORGANIZATION_LINKS.map((link) => <NavItem key={link.href} {...link} onNavigate={onNavigate} />)}
            </NavSection>
          ) : null}
        </Accordion>
      </YStack>
      <Separator />
      <YStack px="$4" pt="$2" gap="$2">
        <Paragraph color="$color11" size="$2" numberOfLines={1}>{session?.user.email}</Paragraph>
        <Button
          size="$3"
          self="flex-start"
          onPress={() => {
            onNavigate();
            router.replace('/');
            void signOut();
          }}
        >
          Sign out
        </Button>
      </YStack>
    </>
  );
}

const styles = StyleSheet.create({
  backdrop: { backgroundColor: '#000' },
  panel: {
    position: 'absolute',
    top: 0,
    bottom: 0,
    left: 0,
    boxShadow: '0 0 24px rgba(0, 0, 0, 0.2)',
  },
});
