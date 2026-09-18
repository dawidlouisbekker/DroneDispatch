import { useEffect, useState } from 'react';
import { router, useLocalSearchParams } from 'expo-router';
import { Button, H1, Paragraph, Spinner, Text, YStack } from 'tamagui';
import { authApi } from '@/api/auth';
import { registerPasskey } from '@/lib/passkeys';

export default function SignupPasskey() {
  const { request } = useLocalSearchParams<{ request?: string }>();
  const [challengeId, setChallengeId] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    authApi.passkeyOptions()
      .then((result) => setChallengeId(result.challenge_id))
      .catch((cause) => setError(cause instanceof Error ? cause.message : 'Passkeys are unavailable'))
      .finally(() => setBusy(false));
  }, []);

  const addPasskey = async () => {
    setError('');
    setBusy(true);
    try {
      await registerPasskey('Personal device');
      await continueRequest(request);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not add a passkey');
    } finally {
      setBusy(false);
    }
  };

  return (
    <YStack flex={1} width="100%" p="$6" gap="$4" items="center" justify="center" bg="$background">
      <YStack gap="$3" maxW={440} width="100%">
        <H1 color="$color12">Add a passkey</H1>
        <Paragraph color="$color11">Use your device biometrics for faster, safer sign-in. This step is optional.</Paragraph>
        {error ? <Text color="$red10">{error}</Text> : null}
        <Button bg="$blue10" color="white" disabled={busy || !challengeId} onPress={() => void addPasskey()}>
          {busy ? <Spinner color="white" /> : 'Add passkey'}
        </Button>
        <Button chromeless disabled={busy} onPress={() => void continueRequest(request)}>Skip for now</Button>
      </YStack>
    </YStack>
  );
}

async function continueRequest(requestId?: string) {
  if (!requestId) return router.replace('/');
  const authRequest = await authApi.request(requestId);
  if (authRequest.next === 'mfa') return router.replace({ pathname: '/mfa', params: { request: requestId } });
  if (authRequest.next === 'mfa_enrollment') return router.replace({ pathname: '/mfa/setup', params: { request: requestId } });
  if (authRequest.next === 'consent') return router.replace({ pathname: '/consent', params: { request: requestId } });
  const result = await authApi.complete(requestId);
  window.location.assign(result.redirect_to);
}
