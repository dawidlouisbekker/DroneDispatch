import { Platform } from 'react-native';

const trimSlash = (url: string) => url.replace(/\/+$/, '');

// Expo inlines EXPO_PUBLIC_* only for direct `process.env.NAME` reads, so read each one explicitly.
const appUrl = trimSlash(process.env.EXPO_PUBLIC_APP_URL ?? 'http://localhost:8086');

export const config = {
  /** Gateway origin: serves the web app, /api/auth, /api/user, /api/merchant and /api/catalog. */
  appUrl,
  /** OAuth issuer: auth-service behind the gateway. */
  authIssuer: trimSlash(process.env.EXPO_PUBLIC_AUTH_ISSUER ?? `${appUrl}/api/auth`),
  awsRegion: process.env.EXPO_PUBLIC_AWS_REGION ?? 'us-west-2',
  mapsApiKey: Platform.OS === 'web' ? process.env.EXPO_PUBLIC_MAPS_KEY_WEB : process.env.EXPO_PUBLIC_MAPS_KEY_NATIVE,
};

/**
 * Base URLs of the APIs. user and merchant are also the token audiences (`resource`); catalog is
 * public and read-only. They default to their ui-gateway paths; set EXPO_PUBLIC_USER_API_URL,
 * EXPO_PUBLIC_MERCHANT_API_URL or EXPO_PUBLIC_CATALOG_API_URL to call a service directly (user and
 * merchant must match that service's API_PUBLIC_URL).
 */
export const apiBase = {
  auth: config.authIssuer,
  user: trimSlash(process.env.EXPO_PUBLIC_USER_API_URL || `${appUrl}/api/user`),
  merchant: trimSlash(process.env.EXPO_PUBLIC_MERCHANT_API_URL || `${appUrl}/api/merchant`),
  catalog: trimSlash(process.env.EXPO_PUBLIC_CATALOG_API_URL || `${appUrl}/api/catalog`),
} as const;
