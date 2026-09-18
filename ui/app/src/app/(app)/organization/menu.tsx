import { OrganizationPlaceholder } from '@/ui/OrganizationPlaceholder';

export default function OrganizationMenu() {
  return (
    <OrganizationPlaceholder
      title="Menu"
      description="Edit menu sections, items, prices and stock."
      endpoint="GET /v1/businesses/{businessId}/menu"
    />
  );
}
