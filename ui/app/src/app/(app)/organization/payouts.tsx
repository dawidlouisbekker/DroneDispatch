import { OrganizationPlaceholder } from '@/ui/OrganizationPlaceholder';

export default function OrganizationPayouts() {
  return (
    <OrganizationPlaceholder
      title="Payouts"
      description="Connect Stripe to receive payouts for completed orders."
      endpoint="GET /v1/businesses/{businessId}/payouts/status"
    />
  );
}
