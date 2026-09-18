import type { Href } from 'expo-router';

export type NavLink = { label: string; href: Extract<Href, string> };

export const SETTINGS_LINKS: readonly NavLink[] = [
  { label: 'Pickup locations', href: '/settings/locations' },
  { label: 'Security', href: '/settings/security' },
  { label: 'Account', href: '/settings/account' },
];

export const ORGANIZATION_LINKS: readonly NavLink[] = [
  { label: 'Orders board', href: '/organization/orders' },
  { label: 'Menu', href: '/organization/menu' },
  { label: 'Pickup station', href: '/organization/pickup-point' },
  { label: 'Payouts', href: '/organization/payouts' },
];

/** Whether `href` is the current screen or one of its ancestors. `/` only matches itself. */
export function isActivePath(pathname: string, href: string): boolean {
  if (href === '/') return pathname === '/';
  return pathname === href || pathname.startsWith(`${href}/`);
}

/** Drawer sections to expand for `pathname`, so the current screen's link is visible. */
export function sectionsForPath(pathname: string): string[] {
  if (pathname.startsWith('/settings/')) return ['settings'];
  if (pathname.startsWith('/organization/')) return ['organization'];
  return [];
}
