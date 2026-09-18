import { useState } from 'react';
import { Link, router, useLocalSearchParams } from 'expo-router';
import { Button, Dialog, H1, Input, Paragraph, Spinner, Text, YStack } from 'tamagui';
import { authApi } from '@/api/auth';

type SignupStep = 'email' | 'otp' | 'password';

export default function SignUp() {
  const { request } = useLocalSearchParams<{ request?: string }>();
  const [step, setStep] = useState<SignupStep>('email');
  const [email, setEmail] = useState('');
  const [challengeId, setChallengeId] = useState('');
  const [otp, setOtp] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [emailChecked, setEmailChecked] = useState(false);
  const [emailTaken, setEmailTaken] = useState(false);

  const startEmailVerification = async () => {
    if (emailChecked || !email.trim()) return;
    setEmailChecked(true);
    setError('');
    setBusy(true);
    try {
      const result = await authApi.checkSignupEmail(email);
      if (result.exists) {
        setEmailTaken(true);
        setError('An account with that email already exists. Sign in instead.');
        return;
      }
      const challenge = await authApi.startSignup(email);
      setChallengeId(challenge.challenge_id);
      setStep('otp');
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  const verifyOtp = async () => {
    setError('');
    setBusy(true);
    try {
      await authApi.verifySignupOtp(challengeId, email, otp);
      setStep('password');
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  const resendOtp = async () => {
    setError('');
    setBusy(true);
    try {
      const challenge = await authApi.resendSignupOtp(challengeId, email);
      setChallengeId(challenge.challenge_id);
      setOtp('');
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  const finish = async () => {
    setError('');
    if (password.length < 12) {
      setError('Use a password of at least 12 characters.');
      return;
    }
    setBusy(true);
    try {
      await authApi.finalizeSignup(challengeId, email, password);
      router.replace({ pathname: '/signup/passkey', params: request ? { request } : undefined });
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <YStack flex={1} width="100%" items="center" justify="center" p="$6" bg="$background">
      <YStack gap="$4" width="100%" maxW={440}>
        <YStack gap="$2">
          <H1 color="$color12" size="$8">Create your account</H1>
          <Paragraph color="$color11">Verify your email before choosing a password.</Paragraph>
        </YStack>

        <YStack gap="$3">
          <Input
            autoCapitalize="none"
            autoComplete="email"
            autoFocus
            borderColor="$color8"
            disabled={step !== 'email'}
            keyboardType="email-address"
            onBlur={() => void startEmailVerification()}
            onChangeText={(value) => {
              setEmail(value);
              setEmailChecked(false);
              setEmailTaken(false);
              setError('');
            }}
            placeholder="Email address"
            value={email}
          />
          {step === 'email' ? (
            <Button
              bg="$blue10"
              color="white"
              disabled={busy || emailTaken || !email.trim()}
              onPress={() => void startEmailVerification()}
            >
              {busy ? <Spinner color="white" /> : 'Continue'}
            </Button>
          ) : null}
          {step === 'password' ? (
            <YStack gap="$3">
              <Text color="$color11">Email verified. Choose a password to finish creating your account.</Text>
              <Input
                autoComplete="new-password"
                borderColor="$color8"
                minLength={12}
                onChangeText={setPassword}
                placeholder="Password"
                secureTextEntry
                value={password}
              />
              <Button bg="$blue10" color="white" disabled={busy} onPress={() => void finish()}>
                {busy ? <Spinner color="white" /> : 'Create account'}
              </Button>
            </YStack>
          ) : null}
          {busy && step === 'email' ? <Spinner color="$blue10" /> : null}
          {error ? <Text color="$red10">{error}</Text> : null}
        </YStack>

        <Paragraph color="$color11">
          Already have an account?{' '}
          <Link href={{ pathname: '/sign-in', params: request ? { request } : undefined }}>Sign in</Link>
        </Paragraph>
      </YStack>

      <Dialog modal open={step === 'otp'} onOpenChange={(open) => !open && setStep('email')}>
        <Dialog.Portal>
          <Dialog.Overlay key="overlay" opacity={0.45} />
          <Dialog.Content bordered elevate key="content" gap="$4" width="90%" maxW={440}>
            <Dialog.Title>Check your email</Dialog.Title>
            <Dialog.Description>Enter the six-digit code sent to {email}.</Dialog.Description>
            <Input
              autoFocus
              autoComplete="one-time-code"
              keyboardType="number-pad"
              maxLength={6}
              onChangeText={setOtp}
              placeholder="000000"
              value={otp}
            />
            {error ? <Text color="$red10">{error}</Text> : null}
            <Button bg="$blue10" color="white" disabled={busy || otp.length !== 6} onPress={() => void verifyOtp()}>
              {busy ? <Spinner color="white" /> : 'Verify email'}
            </Button>
            <Button chromeless disabled={busy} onPress={() => void resendOtp()}>Send a new code</Button>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog>
    </YStack>
  );
}

function message(cause: unknown) {
  return cause instanceof Error ? cause.message : 'Something went wrong';
}