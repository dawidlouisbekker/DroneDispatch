import { router, usePathname } from 'expo-router';
import { Text, XStack } from 'tamagui';
import { isActivePath, NavLink } from '@/lib/nav';

type NavItemProps = NavLink & { onNavigate: () => void };

export function NavItem({ label, href, onNavigate }: NavItemProps) {
  const active = isActivePath(usePathname(), href);
  return (
    <XStack
      role="link"
      cursor="pointer"
      px="$4"
      py="$3"
      rounded="$4"
      bg={active ? '$color4' : 'transparent'}
      hoverStyle={{ bg: active ? '$color4' : '$color3' }}
      pressStyle={{ bg: '$color5' }}
      onPress={() => {
        onNavigate();
        router.navigate(href);
      }}
    >
      <Text color={active ? '$color12' : '$color11'} fontWeight={active ? '600' : '400'}>{label}</Text>
    </XStack>
  );
}
