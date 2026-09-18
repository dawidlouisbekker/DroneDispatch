import { Paragraph, Text } from 'tamagui';
import { useOrganization } from '@/session/OrganizationProvider';
import { Panel } from './Panel';
import { Screen } from './Screen';

type OrganizationPlaceholderProps = { title: string; description: string; endpoint: string };

/** Stand-in for an organization screen that hasn't been built yet. */
export function OrganizationPlaceholder({ title, description, endpoint }: OrganizationPlaceholderProps) {
  const { activeBusiness } = useOrganization();
  return (
    <Screen title={title} description={activeBusiness?.name}>
      <Panel>
        <Paragraph color="$color12">{description}</Paragraph>
        <Text color="$color11" fontSize="$2">Coming soon · {endpoint}</Text>
      </Panel>
    </Screen>
  );
}
