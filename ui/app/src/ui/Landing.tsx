import { Link, router } from 'expo-router';
import { Button, H1, Paragraph, YStack } from 'tamagui';

/** What signed-out visitors see at `/`. */
export function Landing() {
  return (
    <YStack flex={1} p="$6" gap="$4" justify="center" bg="$background">
      <H1 color="$color12">Drone Drop</H1>
      <Paragraph color="$color11">Local commerce, delivered with care.</Paragraph>
      <Button bg="$blue10" color="white" onPress={() => router.push('/sign-in')}>Sign in</Button>
      <Link href="/sign-up" asChild><Button chromeless>Create an account</Button></Link>
    </YStack>
  );
}
