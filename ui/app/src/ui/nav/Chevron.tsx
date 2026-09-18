import { View } from 'tamagui';

/** Dropdown arrow drawn from two borders: points right when closed, down when open. */
export function Chevron({ open }: { open: boolean }) {
  return (
    <View
      width={8}
      height={8}
      mr="$1"
      mt={open ? -4 : 0}
      borderRightWidth={2}
      borderBottomWidth={2}
      borderColor="$color11"
      rotate={open ? '45deg' : '-45deg'}
    />
  );
}
