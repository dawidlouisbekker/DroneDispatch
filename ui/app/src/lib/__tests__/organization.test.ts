/// <reference types="jest" />
import type { Business } from '@/api/types';
import { findBusiness } from '@/lib/organization';

const business = (id: string) => ({ business_id: id, name: `Shop ${id}`, role: 'OWNER' }) as Business;

describe('findBusiness', () => {
  const businesses = [business('a'), business('b')];

  it('returns the business the user switched to', () => {
    expect(findBusiness(businesses, 'b')?.name).toBe('Shop b');
  });

  it('returns null in personal mode', () => {
    expect(findBusiness(businesses, null)).toBeNull();
  });

  it('returns null when the stored business is no longer one of the user’s', () => {
    expect(findBusiness(businesses, 'gone')).toBeNull();
  });
});
