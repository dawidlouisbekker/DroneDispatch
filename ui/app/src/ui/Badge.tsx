import { Text, XStack } from 'tamagui';

export type Tone = 'blue' | 'green' | 'red' | 'gray';

const TONES = {
  blue: { bg: '$blue4', color: '$blue11' },
  green: { bg: '$green4', color: '$green11' },
  red: { bg: '$red4', color: '$red11' },
  gray: { bg: '$color4', color: '$color11' },
} as const;

export function Badge({ tone, children }: { tone: Tone; children: React.ReactNode }) {
  return (
    <XStack self="flex-start" px="$2" py="$1" rounded="$10" bg={TONES[tone].bg}>
      <Text fontSize="$2" fontWeight="600" color={TONES[tone].color}>{children}</Text>
    </XStack>
  );
}
