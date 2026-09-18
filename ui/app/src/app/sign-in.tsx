import { Link, router, useLocalSearchParams } from 'expo-router';
import { Paragraph } from 'tamagui';
import { AuthForm } from '@/ui/AuthForm';
import { authApi } from '@/api/auth';

export default function SignIn() {
  const { request } = useLocalSearchParams<{ request?: string }>();
  const finish = async (email: string, password: string) => {
    const session = await authApi.signIn(email, password);
    if (session.second_factor_pending) {
      router.replace({ pathname: '/mfa', params: request ? { request } : undefined });
      return;
    }
    await continueRequest(request);
  };

  return (
    <AuthForm
      title="Welcome back"
      submitLabel="Sign in"
      onSubmit={finish}
      footer={<Paragraph color="$color11">New here? <Link href={{ pathname: '/sign-up', params: request ? { request } : undefined }}>Create an account</Link></Paragraph>}
    />
  );
}

async function continueRequest(requestId?: string) {
  if (!requestId) {
    router.replace('/');
    return;
  }
  const authRequest = await authApi.request(requestId);
  if (authRequest.next === 'mfa') router.replace({ pathname: '/mfa', params: { request: requestId } });
  else if (authRequest.next === 'mfa_enrollment') router.replace({ pathname: '/mfa/setup', params: { request: requestId } });
  else if (authRequest.next === 'consent') router.replace({ pathname: '/consent', params: { request: requestId } });
  else {
    const result = await authApi.complete(requestId);
    window.location.assign(result.redirect_to);
  }
}