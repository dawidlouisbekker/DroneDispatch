import { useState } from 'react';
import { Button, H1, Input, Paragraph, Spinner, Text, YStack } from 'tamagui';

type AuthFormProps = {
  title: string;
  submitLabel: string;
  footer: React.ReactNode;
  onSubmit: (email: string, password: string) => Promise<void>;
};

export function AuthForm({ title, submitLabel, footer, onSubmit }: AuthFormProps) {
  const [email, setEmail] = useState<string>('');
  const [password, setPassword] = useState<string>('');
  const [error, setError] = useState<string>('');
  const [busy, setBusy] = useState<boolean>(false);

  const submit = async () => {
    setError('');
    setBusy(true);
    try {
      await onSubmit(email, password);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Something went wrong');
    } finally {
      setBusy(false);
    }
  };

  return (
    <YStack flex={1} width="100%" items="center" justify="center" p="$6">
      <YStack gap="$4" width="100%" maxW={440}>
        <YStack gap="$2">
          <H1 color="$color12" size="$8">{title}</H1>
          <Paragraph color="$color11">Secure access to your Drone Drop account.</Paragraph>
        </YStack>
        <YStack gap="$3">
          <Input
            autoCapitalize="none"
            autoComplete="email"
            borderColor="$color8"
            keyboardType="email-address"
            onChangeText={setEmail}
            placeholder="Email address"
            value={email}
          />
          <Input
            autoComplete="new-password"
            borderColor="$color8"
            minLength={12}
            onChangeText={setPassword}
            placeholder="Password"
            secureTextEntry
            value={password}
          />
          {error ? <Text color="$red10">{error}</Text> : null}
          <Button bg="$blue10" color="white" disabled={busy} onPress={() => void submit()}>
            {busy ? <Spinner color="white" /> : submitLabel}
          </Button>
        </YStack>
        {footer}
      </YStack>
    </YStack>
  );
}