import { Stack } from 'expo-router';
import { TamaguiProvider } from 'tamagui';
import { SessionProvider } from '@/session/SessionProvider';
import tamaguiConfig from '../../tamagui.config';

export default function RootLayout() {
  return (
    <TamaguiProvider config={tamaguiConfig} defaultTheme="light">
      <SessionProvider>
        <Stack screenOptions={{ headerShown: false }} />
      </SessionProvider>
    </TamaguiProvider>
  );
}
