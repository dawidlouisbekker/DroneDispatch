import { Accordion, Text, YStack } from 'tamagui';
import { Chevron } from './Chevron';

type NavSectionProps = {
  value: string;
  label: string;
  subtitle?: string;
  /** Whether the parent Accordion has this section expanded; drives the chevron. */
  open: boolean;
  children: React.ReactNode;
};

/** A collapsible group of drawer links. Render inside `<Accordion type="multiple">`. */
export function NavSection({ value, label, subtitle, open, children }: NavSectionProps) {
  return (
    <Accordion.Item value={value}>
      <Accordion.Trigger
        unstyled
        flexDirection="row"
        items="center"
        justify="space-between"
        cursor="pointer"
        px="$4"
        py="$3"
        rounded="$4"
        hoverStyle={{ bg: '$color3' }}
        pressStyle={{ bg: '$color5' }}
      >
        <YStack>
          <Text color="$color12" fontWeight="600">{label}</Text>
          {subtitle ? <Text color="$color11" fontSize="$2">{subtitle}</Text> : null}
        </YStack>
        <Chevron open={open} />
      </Accordion.Trigger>
      <Accordion.Content unstyled pl="$3" gap="$1">
        {children}
      </Accordion.Content>
    </Accordion.Item>
  );
}
