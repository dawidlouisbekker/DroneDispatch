import { useCallback, useEffect, useRef, useState } from 'react';
import { router, useLocalSearchParams } from 'expo-router';
import { Button, H1, Paragraph, Spinner, Text, YStack } from 'tamagui';
import { authApi } from '@/api/auth';
import { verifyWithPasskey } from '@/lib/passkeys';

export default function Mfa() {
  const { request } = useLocalSearchParams<{ request?: string }>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const prompted = useRef(false);

  const verify = useCallback(
    async (automatic: boolean) => {
      setError('');
      setBusy(true);
      try {
        await verifyWithPasskey();
        if (request) {
          const authRequest = await authApi.request(request);
          if (authRequest.next === 'consent') router.replace({ pathname: '/consent', params: { request } });
          else {
            const result = await authApi.complete(request);
            window.location.assign(result.redirect_to);
          }
        } else {
          router.replace('/');
        }
      } catch (cause) {
        // Browsers may refuse a prompt that no tap started; the button below is the retry.
        if (automatic && cause instanceof Error && cause.name === 'NotAllowedError') return;
        setError(cause instanceof Error ? cause.message : 'Your passkey was not accepted');
      } finally {
        setBusy(false);
      }
    },
    [request],
  );

  useEffect(() => {
    if (prompted.current) return;
    prompted.current = true;
    void verify(true);
  }, [verify]);

  const switchAccount = async () => {
    await authApi.signOut().catch(() => undefined);
    router.replace({ pathname: '/sign-in', params: request ? { request } : undefined });
  };

  return (
    <YStack flex={1} width="100%" p="$6" gap="$4" items="center" justify="center" bg="$background">
      <YStack gap="$2" maxW={440} width="100%">
        <H1 color="$color12">Verify your identity</H1>
        <Paragraph color="$color11">Use your passkey to finish signing in.</Paragraph>
        {error ? <Text color="$red10">{error}</Text> : null}
        <Button bg="$blue10" color="white" disabled={busy} onPress={() => void verify(false)}>
          {busy ? <Spinner color="white" /> : 'Use passkey'}
        </Button>
        <Button chromeless disabled={busy} onPress={() => void switchAccount()}>
          Sign in with a different account
        </Button>
      </YStack>
    </YStack>
  );
}
