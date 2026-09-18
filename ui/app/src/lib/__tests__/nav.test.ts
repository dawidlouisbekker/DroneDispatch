/// <reference types="jest" />
import { isActivePath, sectionsForPath } from '@/lib/nav';

describe('isActivePath', () => {
  it('matches the root only on the root', () => {
    expect(isActivePath('/', '/')).toBe(true);
    expect(isActivePath('/settings/account', '/')).toBe(false);
  });

  it('matches a screen and the screens below it', () => {
    expect(isActivePath('/settings/account', '/settings/account')).toBe(true);
    expect(isActivePath('/organization/menu/items', '/organization/menu')).toBe(true);
  });

  it('does not match a sibling that shares a prefix', () => {
    expect(isActivePath('/settings/accounts', '/settings/account')).toBe(false);
  });
});

describe('sectionsForPath', () => {
  it('expands the section holding the current screen', () => {
    expect(sectionsForPath('/settings/security')).toEqual(['settings']);
    expect(sectionsForPath('/organization/payouts')).toEqual(['organization']);
    expect(sectionsForPath('/')).toEqual([]);
  });
});
