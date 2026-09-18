import { H2, Paragraph, ScrollView, Text, YStack } from 'tamagui';

type ScreenProps = { title: string; description?: string; children?: React.ReactNode };

/** Scrollable page body for screens inside the app shell. */
export function Screen({ title, description, children }: ScreenProps) {
  return (
    <ScrollView flex={1} bg="$background" contentContainerStyle={{ items: 'center' }}>
      <YStack width="100%" maxW={720} p="$5" gap="$5">
        <YStack gap="$1">
          <H2 color="$color12" size="$8">{title}</H2>
          {description ? <Paragraph color="$color11">{description}</Paragraph> : null}
        </YStack>
        {children}
      </YStack>
    </ScrollView>
  );
}

export function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <Text color="$color11" fontSize="$3" fontWeight="600" textTransform="uppercase" letterSpacing={0.5}>
      {children}
    </Text>
  );
}
