import type { Order, OrderState } from '@/api/types';
import { isActiveOrder } from '@/lib/format';

const SUCCEEDED: ReadonlySet<OrderState> = new Set(['DELIVERED', 'COMPLETED']);
const FAILED: ReadonlySet<OrderState> = new Set(['PAYMENT_FAILED', 'CANCELLED', 'REFUNDED']);

const newestFirst = (a: Order, b: Order) => Date.parse(b.created_at) - Date.parse(a.created_at);

/** Splits orders into in-progress (`current`) and finished (`previous`), each newest first. */
export function splitOrders(orders: readonly Order[]): { current: Order[]; previous: Order[] } {
  const current: Order[] = [];
  const previous: Order[] = [];
  for (const order of orders) {
    (isActiveOrder(order.state) ? current : previous).push(order);
  }
  return { current: current.sort(newestFirst), previous: previous.sort(newestFirst) };
}

/** Badge colour for an order state. */
export function orderTone(state: OrderState): 'blue' | 'green' | 'red' | 'gray' {
  if (isActiveOrder(state)) return 'blue';
  if (SUCCEEDED.has(state)) return 'green';
  if (FAILED.has(state)) return 'red';
  return 'gray';
}
