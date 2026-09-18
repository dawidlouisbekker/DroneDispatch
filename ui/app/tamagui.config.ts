import { createTamagui } from 'tamagui';
import { defaultConfig } from '@tamagui/config/v5';

export const tamaguiConfig = createTamagui(defaultConfig);

export type AppTamaguiConfig = typeof tamaguiConfig;

declare module 'tamagui' {
  interface TamaguiCustomConfig extends AppTamaguiConfig {}
}

export default tamaguiConfig;