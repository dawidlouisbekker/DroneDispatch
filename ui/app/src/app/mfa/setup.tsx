import { useState } from 'react';
import { router, useLocalSearchParams } from 'expo-router';
import { Button, H1, Paragraph, Spinner, Text, YStack } from 'tamagui';
import { authApi } from '@/api/auth';
import { registerPasskey } from '@/lib/passkeys';

export default function MfaSetup() {
  const { request } = useLocalSearchParams<{ request?: string }>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const addPasskey = async () => {
    setError('');
    setBusy(true);
    try {
      await registerPasskey('Personal device');
      await finishRequest(request);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not add a passkey');
    } finally {
      setBusy(false);
    }
  };

  return (
    <YStack flex={1} width="100%" p="$6" gap="$4" items="center" justify="center" bg="$background">
      <YStack gap="$2" maxW={440} width="100%">
        <H1 color="$color12">Add a passkey to continue</H1>
        <Paragraph color="$color11">
          This needs two-step verification. Add a passkey that unlocks with the fingerprint, face or screen lock on
          this device.
        </Paragraph>
        {error ? <Text color="$red10">{error}</Text> : null}
        <Button bg="$blue10" color="white" disabled={busy} onPress={() => void addPasskey()}>
          {busy ? <Spinner color="white" /> : 'Add passkey'}
        </Button>
      </YStack>
    </YStack>
  );
}

async function finishRequest(requestId?: string) {
  if (!requestId) return router.replace('/');
  const authRequest = await authApi.request(requestId);
  if (authRequest.next === 'consent') return router.replace({ pathname: '/consent', params: { request: requestId } });
  const result = await authApi.complete(requestId);
  window.location.assign(result.redirect_to);
}
