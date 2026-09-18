import type { ConfigContext, ExpoConfig } from 'expo/config';

// Runtime settings come from EXPO_PUBLIC_* variables (see .env.example), not from here.
export default ({ config }: ConfigContext): ExpoConfig => ({
  ...config,
  name: 'Drone Drop',
  slug: 'drone-drop',
  version: '0.1.0',
  orientation: 'portrait',
  icon: './assets/images/icon.png',
  scheme: 'dronedrop',
  userInterfaceStyle: 'light',
  ios: {
    bundleIdentifier: 'dev.dronedrop.app',
    icon: './assets/expo.icon',
  },
  android: {
    package: 'dev.dronedrop.app',
    adaptiveIcon: {
      backgroundColor: '#E6F4FE',
      foregroundImage: './assets/images/android-icon-foreground.png',
      backgroundImage: './assets/images/android-icon-background.png',
      monochromeImage: './assets/images/android-icon-monochrome.png',
    },
    predictiveBackGestureEnabled: false,
  },
  web: {
    // A single-page app: the gateway serves index.html for every route.
    output: 'single',
    favicon: './assets/images/favicon.png',
  },
  plugins: [
    'expo-router',
    [
      'expo-splash-screen',
      {
        backgroundColor: '#208AEF',
        image: './assets/images/splash-icon.png',
        imageWidth: 76,
      },
    ],
    '@maplibre/maplibre-react-native',
    'expo-secure-store',
    [
      'expo-image-picker',
      {
        cameraPermission: 'Drone Drop uses the camera to photograph the pickup marker on your pad.',
        photosPermission: 'Drone Drop lets you choose an existing photo of your pickup marker.',
      },
    ],
  ],
  experiments: {
    typedRoutes: true,
    reactCompiler: true,
  },
});
