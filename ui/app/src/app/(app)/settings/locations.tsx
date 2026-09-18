import { useEffect, useState } from 'react';
import { Paragraph, Spinner, Text } from 'tamagui';
import { errorMessage, userApi } from '@/api/clients';
import type { PickupLocation } from '@/api/types';
import { Badge, Tone } from '@/ui/Badge';
import { Panel } from '@/ui/Panel';
import { Screen } from '@/ui/Screen';

const STATUS: Record<PickupLocation['status'], { label: string; tone: Tone }> = {
  PENDING: { label: 'Pending verification', tone: 'gray' },
  VERIFIED: { label: 'Verified', tone: 'green' },
  REVOKED: { label: 'Revoked', tone: 'red' },
};

export default function PickupLocations() {
  const [locations, setLocations] = useState<PickupLocation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    userApi
      .GET('/v1/pickup-locations')
      .then(({ data, error: problem }) => {
        if (!data) throw problem;
        setLocations(data.locations);
      })
      .catch((cause: unknown) => setError(errorMessage(cause, 'Could not load your pickup locations')))
      .finally(() => setLoading(false));
  }, []);

  return (
    <Screen title="Pickup locations" description="Places drones deliver your orders to.">
      {loading ? <Spinner color="$blue10" self="flex-start" /> : null}
      {error ? <Text color="$red10">{error}</Text> : null}
      {!loading && !error && locations.length === 0 ? (
        <Paragraph color="$color11">No pickup locations yet.</Paragraph>
      ) : null}
      {locations.map((location) => (
        <Panel key={location.pickup_location_id}>
          <Text color="$color12" fontWeight="600" textTransform="capitalize">{location.label}</Text>
          <Text color="$color11" fontSize="$2">{location.address}</Text>
          <Badge tone={STATUS[location.status].tone}>{STATUS[location.status].label}</Badge>
        </Panel>
      ))}
    </Screen>
  );
}
