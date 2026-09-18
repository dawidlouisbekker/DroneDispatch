import { Href, router } from 'expo-router';
import { Button, Text, YStack } from 'tamagui';
import { useSession } from '@/session/SessionProvider';
import { Panel } from '@/ui/Panel';
import { Screen } from '@/ui/Screen';

export default function Security() {
  const { session } = useSession();
  if (!session) return null;
  const { factors } = session;

  return (
    <Screen
      title="Security"
      description="Passkeys protect your account and are required to add new drop-off locations."
    >
      <FactorRow
        title="Passkeys"
        detail={factors.passkeys === 1 ? '1 passkey' : `${factors.passkeys} passkeys`}
        action={{ label: 'Add a passkey', href: '/signup/passkey' }}
      />
    </Screen>
  );
}

type FactorRowProps = {
  title: string;
  detail?: string;
  status?: React.ReactNode;
  action?: { label: string; href: Href };
};

function FactorRow({ title, detail, status, action }: FactorRowProps) {
  return (
    <Panel flexDirection="row" items="center" justify="space-between" gap="$3">
      <YStack gap="$1" shrink={1}>
        <Text color="$color12" fontWeight="600">{title}</Text>
        {detail ? <Text color="$color11" fontSize="$2">{detail}</Text> : null}
        {status}
      </YStack>
      {action ? <Button size="$3" onPress={() => router.push(action.href)}>{action.label}</Button> : null}
    </Panel>
  );
}
