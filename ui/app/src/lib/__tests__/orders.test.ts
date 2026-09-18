/// <reference types="jest" />
import type { Order, OrderState } from '@/api/types';
import { orderTone, splitOrders } from '@/lib/orders';

const money = { amount_cents: 1250, currency: 'usd' };

function order(id: string, state: OrderState, createdAt: string): Order {
  return {
    order_id: id,
    place_id: 'place-1',
    business_id: 'b1',
    business_name: 'Corner Cafe',
    channel: 'MCP',
    state,
    items: [],
    subtotal: money,
    delivery_fee: money,
    total: money,
    pickup_location: {
      pickup_location_id: 'l1',
      label: 'home',
      address: '1 Pike St',
      position: { lat: 47.6, lon: -122.3 },
      position_source: 'MAP_PIN',
      status: 'VERIFIED',
    },
    timeline: [],
    created_at: createdAt,
  };
}

const ids = (orders: Order[]) => orders.map((o) => o.order_id);

describe('splitOrders', () => {
  it('puts in-progress orders in current and finished orders in previous', () => {
    const { current, previous } = splitOrders([
      order('flying', 'PICKED_UP', '2026-09-15T10:00:00Z'),
      order('delivered', 'DELIVERED', '2026-09-14T10:00:00Z'),
      order('waiting', 'AWAITING_MERCHANT', '2026-09-15T09:00:00Z'),
      order('refunded', 'REFUNDED', '2026-09-13T10:00:00Z'),
      order('failed', 'PAYMENT_FAILED', '2026-09-12T10:00:00Z'),
    ]);
    expect(ids(current)).toEqual(['flying', 'waiting']);
    expect(ids(previous)).toEqual(['delivered', 'refunded', 'failed']);
  });

  it('counts voice orders waiting for approval as in progress', () => {
    const { current, previous } = splitOrders([order('approval', 'AWAITING_APPROVAL', '2026-09-15T10:00:00Z')]);
    expect(ids(current)).toEqual(['approval']);
    expect(previous).toEqual([]);
  });

  it('sorts both lists newest first', () => {
    const { current, previous } = splitOrders([
      order('old-active', 'PAID', '2026-09-01T00:00:00Z'),
      order('old-done', 'COMPLETED', '2026-09-01T00:00:00Z'),
      order('new-active', 'DRONE_ASSIGNED', '2026-09-10T00:00:00Z'),
      order('new-done', 'CANCELLED', '2026-09-10T00:00:00Z'),
    ]);
    expect(ids(current)).toEqual(['new-active', 'old-active']);
    expect(ids(previous)).toEqual(['new-done', 'old-done']);
  });
});

describe('orderTone', () => {
  it('colours active, successful and failed orders', () => {
    expect(orderTone('AT_PICKUP')).toBe('blue');
    expect(orderTone('AWAITING_APPROVAL')).toBe('blue');
    expect(orderTone('COMPLETED')).toBe('green');
    expect(orderTone('CANCELLED')).toBe('red');
  });
});
