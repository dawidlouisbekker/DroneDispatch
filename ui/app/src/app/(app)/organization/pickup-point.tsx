import { OrganizationPlaceholder } from '@/ui/OrganizationPlaceholder';

export default function OrganizationPickupPoint() {
  return (
    <OrganizationPlaceholder
      title="Pickup station"
      description="Register the station at your pickup pad so drones can find it and verify it's yours."
      endpoint="GET /v1/businesses/{businessId}/station"
    />
  );
}
