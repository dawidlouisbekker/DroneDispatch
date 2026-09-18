import { router, useLocalSearchParams } from 'expo-router';
import { Button, H1, Paragraph, Text, YStack } from 'tamagui';
import { useEffect, useState } from 'react';
import { authApi, AuthRequest } from '@/api/auth';

export default function Consent() {
  const { request } = useLocalSearchParams<{ request: string }>();
  const [authRequest, setAuthRequest] = useState<AuthRequest>();
  const [error, setError] = useState('');
  useEffect(() => {
    if (request) authApi.request(request).then(setAuthRequest).catch((cause) => setError(cause instanceof Error ? cause.message : 'Request expired'));
  }, [request]);

  const approve = async () => {
    if (!request) return;
    const result = await authApi.complete(request, true);
    window.location.assign(result.redirect_to);
  };

  return (
    <YStack flex={1} p="$6" gap="$4" justify="center" bg="$background">
      <YStack gap="$3" maxW={440} width="100%">
        <H1 color="$color12">Allow access?</H1>
        {authRequest ? <Paragraph color="$color11">{authRequest.client.name} wants access to {authRequest.redirect_host}.</Paragraph> : null}
        {authRequest ? <Text color="$color11">Permissions: {authRequest.scopes.join(', ') || 'basic account access'}</Text> : null}
        {error ? <Text color="$red10">{error}</Text> : null}
        <Button bg="$blue10" color="white" disabled={!authRequest} onPress={() => void approve()}>Allow</Button>
        <Button chromeless onPress={() => router.replace('/')}>Deny</Button>
      </YStack>
    </YStack>
  );
}