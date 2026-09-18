import { useEffect, useState } from 'react';
import { Button, Paragraph, Spinner, Text, XStack, YStack } from 'tamagui';
import { errorMessage, userApi } from '@/api/clients';
import type { Order } from '@/api/types';
import {
  formatCountdown,
  formatDateTime,
  formatEta,
  formatMoney,
  isActiveOrder,
  orderStateLabel,
} from '@/lib/format';
import { orderTone, splitOrders } from '@/lib/orders';
import { Badge } from '@/ui/Badge';
import { Panel } from '@/ui/Panel';
import { Screen, SectionTitle } from '@/ui/Screen';

const PAGE_SIZE = 20;

async function fetchOrders(cursor?: string) {
  const { data, error } = await userApi.GET('/v1/orders', { params: { query: { cursor, limit: PAGE_SIZE } } });
  if (!data) throw error;
  return { orders: data.orders, nextCursor: data.next_cursor ?? null };
}

export default function Orders() {
  const [orders, setOrders] = useState<Order[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    fetchOrders()
      .then((page) => {
        setOrders(page.orders);
        setNextCursor(page.nextCursor);
      })
      .catch((cause: unknown) => setError(errorMessage(cause, 'Could not load your orders')))
      .finally(() => setLoading(false));
  }, []);

  const loadMore = async () => {
    if (!nextCursor) return;
    setError('');
    setLoadingMore(true);
    try {
      const page = await fetchOrders(nextCursor);
      setOrders((loaded) => [...loaded, ...page.orders]);
      setNextCursor(page.nextCursor);
    } catch (cause) {
      setError(errorMessage(cause, 'Could not load more orders'));
    } finally {
      setLoadingMore(false);
    }
  };

  const { current, previous } = splitOrders(orders);
  const failed = Boolean(error) && orders.length === 0;

  return (
    <Screen title="Orders" description="Track deliveries in progress and look back at past orders.">
      {loading ? <Spinner color="$blue10" self="flex-start" /> : null}
      {error ? <Text color="$red10">{error}</Text> : null}
      {!loading && !failed ? (
        <>
          <OrderSection title="Current orders" empty="No orders in progress." orders={current} />
          <OrderSection title="Previous orders" empty="No previous orders." orders={previous} />
          {nextCursor ? (
            <Button chromeless self="flex-start" disabled={loadingMore} onPress={() => void loadMore()}>
              {loadingMore ? <Spinner color="$blue10" /> : 'Load more'}
            </Button>
          ) : null}
        </>
      ) : null}
    </Screen>
  );
}

function OrderSection({ title, empty, orders }: { title: string; empty: string; orders: Order[] }) {
  return (
    <YStack gap="$3">
      <SectionTitle>{title}</SectionTitle>
      {orders.length === 0 ? (
        <Paragraph color="$color11">{empty}</Paragraph>
      ) : (
        orders.map((order) => <OrderRow key={order.order_id} order={order} />)
      )}
    </YStack>
  );
}

function OrderRow({ order }: { order: Order }) {
  return (
    <Panel>
      <XStack justify="space-between" gap="$3">
        <Text color="$color12" fontWeight="600" shrink={1}>{order.business_name}</Text>
        <Text color="$color12">{formatMoney(order.total)}</Text>
      </XStack>
      <Text color="$color11" fontSize="$2">{formatDateTime(order.created_at)}</Text>
      <XStack items="center" gap="$3" flexWrap="wrap">
        <Badge tone={orderTone(order.state)}>{orderStateLabel(order.state)}</Badge>
        {order.state === 'AWAITING_MERCHANT' && order.merchant_accept_by ? (
          <AcceptCountdown until={order.merchant_accept_by} />
        ) : null}
        {isActiveOrder(order.state) && order.eta_seconds != null ? (
          <Text color="$color11" fontSize="$2">Arrives in {formatEta(order.eta_seconds)}</Text>
        ) : null}
      </XStack>
    </Panel>
  );
}

function AcceptCountdown({ until }: { until: string }) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return <Text color="$color11" fontSize="$2">Shop has {formatCountdown(until, now)} to accept</Text>;
}
