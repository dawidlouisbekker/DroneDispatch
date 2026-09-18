import { styled, YStack } from 'tamagui';

/** Bordered card for list rows. */
export const Panel = styled(YStack, {
  gap: '$2',
  p: '$4',
  rounded: '$4',
  borderWidth: 1,
  borderColor: '$borderColor',
});
