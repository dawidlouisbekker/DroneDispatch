import { router } from 'expo-router';
import { Button, Paragraph, Spinner, Text, YStack } from 'tamagui';
import { useOrganization } from '@/session/OrganizationProvider';
import { useSession } from '@/session/SessionProvider';
import { Badge } from '@/ui/Badge';
import { Panel } from '@/ui/Panel';
import { Screen, SectionTitle } from '@/ui/Screen';

export default function Account() {
  const { session, signOut } = useSession();
  const { businesses, status, error, activeBusiness, switchTo, leave } = useOrganization();

  const switchToBusiness = async (businessId: string) => {
    await switchTo(businessId);
    router.navigate('/organization/orders');
  };

  return (
    <Screen title="Account">
      <YStack gap="$1">
        <SectionTitle>Signed in as</SectionTitle>
        <Text color="$color12" fontSize="$5" fontWeight="600">{session?.user.email}</Text>
      </YStack>

      <YStack gap="$3">
        <SectionTitle>Organization</SectionTitle>
        <Paragraph color="$color11">
          {activeBusiness
            ? `You're working as ${activeBusiness.name}. Its tools are under Organization in the menu.`
            : 'Switch to an organization to manage its orders, menu, pickup point and payouts.'}
        </Paragraph>
        {status === 'loading' ? <Spinner color="$blue10" self="flex-start" /> : null}
        {status === 'error' ? <Text color="$red10">{error}</Text> : null}
        {status === 'ready' && businesses.length === 0 ? (
          <Paragraph color="$color11">You aren't a member of any organization yet.</Paragraph>
        ) : null}
        {businesses.map((business) => {
          const active = business.business_id === activeBusiness?.business_id;
          return (
            <Panel
              key={business.business_id}
              flexDirection="row"
              items="center"
              justify="space-between"
              gap="$3"
              borderColor={active ? '$blue8' : '$borderColor'}
            >
              <YStack gap="$1" shrink={1}>
                <Text color="$color12" fontWeight="600">{business.name}</Text>
                <Text color="$color11" fontSize="$2">
                  {business.role === 'OWNER' ? 'Owner' : 'Staff'} · {business.address}
                </Text>
              </YStack>
              {active ? (
                <Badge tone="blue">Active</Badge>
              ) : (
                <Button size="$3" onPress={() => void switchToBusiness(business.business_id)}>Switch</Button>
              )}
            </Panel>
          );
        })}
        {activeBusiness ? (
          <Button chromeless self="flex-start" onPress={() => void leave()}>Switch back to personal</Button>
        ) : null}
      </YStack>

      <Button
        self="flex-start"
        onPress={() => {
          router.replace('/');
          void signOut();
        }}
      >
        Sign out
      </Button>
    </Screen>
  );
}
