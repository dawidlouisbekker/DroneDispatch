import { startAuthentication, startRegistration } from '@simplewebauthn/browser';
import { authApi, Session } from '@/api/auth';

type RegistrationOptions = Parameters<typeof startRegistration>[0]['optionsJSON'];
type AuthenticationOptions = Parameters<typeof startAuthentication>[0]['optionsJSON'];

/** Creates a passkey on this device and adds it to the signed-in account. */
export async function registerPasskey(name: string): Promise<Session> {
  const { challenge_id, options } = await authApi.passkeyOptions();
  const credential = await startRegistration({ optionsJSON: options as unknown as RegistrationOptions });
  return authApi.addPasskey(challenge_id, name, credential as unknown as Record<string, unknown>);
}

/** Checks one of the signed-in account's passkeys: the second factor after a password. */
export async function verifyWithPasskey(): Promise<Session> {
  const { challenge_id, options } = await authApi.secondFactorOptions();
  const credential = await startAuthentication({ optionsJSON: options as unknown as AuthenticationOptions });
  return authApi.verifyPasskey(challenge_id, credential as unknown as Record<string, unknown>);
}
