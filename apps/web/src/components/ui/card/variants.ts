import type { VariantProps } from 'class-variance-authority'
import { cva } from 'class-variance-authority'

export const cardVariants = cva('', {
  variants: {
    variant: {
      none: 'pb-12',
      default: 'rounded-panel border border-hairline bg-card p-5 text-card-foreground max-sm:px-4',
      panel: 'rounded-panel border border-hairline bg-card p-5 text-card-foreground max-sm:px-4',
      surface: 'rounded-panel border border-hairline bg-surface p-5 max-sm:px-4',
      outline: 'rounded-panel border border-hairline p-5 max-sm:px-4',
      accent: 'rounded-xl p-6 max-sm:px-4 bg-accent text-accent-foreground shadow-soft',
      warning: 'rounded-xl p-6 max-sm:px-4 bg-warning-background/20 text-warning shadow-sm',
      destructive:
        'rounded-xl p-6 max-sm:px-4 bg-destructive-background/20 text-destructive shadow-sm',
      destructiveOutline: 'rounded-xl p-6 max-sm:px-4 border border-destructive text-destructive',
    },
  },
  defaultVariants: {
    variant: 'default',
  },
})

export type CardVariants = VariantProps<typeof cardVariants>
