import { Platform } from 'react-native';
import * as SecureStore from 'expo-secure-store';

// Small key-value store for UI preferences: SecureStore on native, localStorage on web
// (which can throw in private windows or when site data is blocked).
export const storage = {
  async get(key: string): Promise<string | null> {
    if (Platform.OS !== 'web') return SecureStore.getItemAsync(key);
    try {
      return globalThis.localStorage?.getItem(key) ?? null;
    } catch {
      return null;
    }
  },
  async set(key: string, value: string): Promise<void> {
    if (Platform.OS !== 'web') return SecureStore.setItemAsync(key, value);
    try {
      globalThis.localStorage?.setItem(key, value);
    } catch {
      // Storage unavailable: the value lasts until the page reloads.
    }
  },
  async remove(key: string): Promise<void> {
    if (Platform.OS !== 'web') return SecureStore.deleteItemAsync(key);
    try {
      globalThis.localStorage?.removeItem(key);
    } catch {
      // Storage unavailable: nothing was stored.
    }
  },
};
