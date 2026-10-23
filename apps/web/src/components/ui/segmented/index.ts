import { cva, type VariantProps } from 'class-variance-authority'

/**
 * The one segmented control look: a filled track with the chosen option set into it in the
 * canvas colour, so it reads as a cut-out of the page. Shared by `ChoiceGroup`, `Tabs` and the toggle groups, which mark
 * the chosen option with `aria-checked`, `data-state=on` or `data-state=active`.
 */
export const segmentedTrack = 'inline-flex max-w-full flex-wrap gap-0.5 rounded-control bg-fill p-1'

const raised = [
  'aria-checked:bg-surface aria-checked:text-primary aria-checked:shadow-soft-sm aria-checked:ring-1 aria-checked:ring-hairline aria-checked:ring-inset',
  'data-[state=on]:bg-surface data-[state=on]:text-primary data-[state=on]:shadow-soft-sm data-[state=on]:ring-1 data-[state=on]:ring-hairline data-[state=on]:ring-inset',
  'data-[state=active]:bg-surface data-[state=active]:text-primary data-[state=active]:shadow-soft-sm data-[state=active]:ring-1 data-[state=active]:ring-hairline data-[state=active]:ring-inset',
].join(' ')

export const segmentedItem = cva(
  `inline-flex cursor-pointer items-center justify-center gap-1.5 rounded-lg font-medium whitespace-nowrap text-muted transition-colors hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 ${raised}`,
  {
    variants: {
      size: {
        sm: 'h-7 px-2.5 text-[0.875rem] max-sm:min-h-8',
        md: 'h-8 px-4 text-[0.90625rem] max-sm:px-3',
      },
    },
    defaultVariants: { size: 'sm' },
  }
)

export type SegmentedItemVariants = VariantProps<typeof segmentedItem>
