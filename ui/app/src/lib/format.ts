import type { BoardOrderStatus, DroneState, Money, OrderState } from '@/api/types';

export function formatMoney(money: Money): string {
  return new Intl.NumberFormat('en-US', { style: 'currency', currency: money.currency.toUpperCase() }).format(
    money.amount_cents / 100,
  );
}

export function formatDateTime(iso: string): string {
  return new Date(iso).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });
}

/** `m:ss` until `iso`, or `0:00` once it has passed. */
export function formatCountdown(iso: string, now: number = Date.now()): string {
  const seconds = Math.max(0, Math.round((new Date(iso).getTime() - now) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

export function formatEta(seconds: number | null | undefined): string {
  if (seconds == null) return '—';
  if (seconds < 60) return 'under a minute';
  return `${Math.round(seconds / 60)} min`;
}

const ORDER_STATE_LABELS: Record<OrderState, string> = {
  AWAITING_APPROVAL: 'Waiting for your approval',
  AUTHORIZING: 'Authorizing payment',
  AWAITING_MERCHANT: 'Waiting for the shop',
  CAPTURING: 'Charging your card',
  PAID: 'Paid',
  DISPATCH_REQUESTED: 'Finding a drone',
  DRONE_ASSIGNED: 'Drone on the way to the shop',
  AT_PICKUP: 'Drone at the shop',
  PICKED_UP: 'On the way to you',
  DELIVERED: 'Delivered',
  COMPLETED: 'Completed',
  PAYMENT_FAILED: 'Payment failed',
  CANCELLED: 'Cancelled',
  REFUNDED: 'Refunded',
};

export const orderStateLabel = (state: OrderState) => ORDER_STATE_LABELS[state];

const ACTIVE_STATES: ReadonlySet<OrderState> = new Set([
  'AWAITING_APPROVAL',
  'AUTHORIZING',
  'AWAITING_MERCHANT',
  'CAPTURING',
  'PAID',
  'DISPATCH_REQUESTED',
  'DRONE_ASSIGNED',
  'AT_PICKUP',
  'PICKED_UP',
]);

export const isActiveOrder = (state: OrderState) => ACTIVE_STATES.has(state);

const DRONE_STATE_LABELS: Record<DroneState, string> = {
  IDLE: 'Idle',
  TAKEOFF: 'Taking off',
  TO_PICKUP: 'Flying to the shop',
  APPROACH: 'Looking for the station',
  HANDSHAKE: 'Verifying the station',
  LOADING: 'Loading',
  TO_DROPOFF: 'Flying to you',
  DROPOFF: 'Dropping off',
  RETURNING: 'Returning to dock',
  CHARGING: 'Charging',
  OUT_OF_SERVICE: 'Out of service',
};

export const droneStateLabel = (state: DroneState) => DRONE_STATE_LABELS[state];

const BOARD_STATUS_LABELS: Record<BoardOrderStatus, string> = {
  AWAITING_DECISION: 'Needs a decision',
  ACCEPTED: 'Accepted',
  REJECTED: 'Rejected',
  EXPIRED: 'Expired',
  CANCELLED: 'Cancelled',
  LOADED: 'Loaded onto drone',
};

export const boardStatusLabel = (status: BoardOrderStatus) => BOARD_STATUS_LABELS[status];
