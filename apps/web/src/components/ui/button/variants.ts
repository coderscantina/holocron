import { cva, type VariantProps } from 'class-variance-authority'

export const buttonVariants = cva(
  'cursor-pointer shrink-0 inline-flex items-center justify-center gap-1.5 whitespace-nowrap font-medium transition-colors duration-300 ease-butter focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        default: 'bg-secondary text-secondary-foreground shadow-sm hover:bg-secondary/80',
        input: 'bg-input text-secondary-foreground shadow-sm hover:bg-input/80',
        primary: 'bg-primary text-primary-foreground shadow hover:bg-primary/80',
        accent:
          'bg-accent text-accent-foreground shadow-sm hover:bg-accent-hover shadow-lg shadow-accent/20',
        destructive:
          'bg-destructive-background/20 text-destructive shadow-sm hover:bg-destructive-background/30',
        warning: 'bg-warning-background/20 text-warning shadow-sm hover:bg-warning-background/80',
        ai: 'bg-ai-background/20 text-ai shadow-sm hover:bg-ai-background/80',
        outline: 'border border-hairline bg-card hover:bg-fill',
        fill: 'bg-fill text-primary hover:bg-fill-strong data-[state=open]:bg-fill-strong',
        ghost: 'hover:bg-secondary/80',
        link: 'text-primary underline-offset-4 hover:underline !px-0',
      },
      size: {
        default: 'h-8 px-3 rounded-md',
        xs: 'h-7 rounded-md px-2 text-xs',
        sm: 'h-7 rounded-md px-2.5 text-sm',
        lg: 'h-9 rounded-md px-5',
        control: 'h-shell-control gap-2 rounded-control px-3.5 text-sm',
        icon: 'size-8 rounded-md',
        toolbar: 'size-7 rounded-md px-2 text-xs',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  }
)

export type ButtonVariants = VariantProps<typeof buttonVariants>
