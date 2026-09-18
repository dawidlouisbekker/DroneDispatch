import { OrganizationPlaceholder } from '@/ui/OrganizationPlaceholder';

export default function OrganizationOrders() {
  return (
    <OrganizationPlaceholder
      title="Orders board"
      description="Accept or reject incoming orders and mark them loaded for pickup."
      endpoint="GET /v1/businesses/{businessId}/orders"
    />
  );
}
