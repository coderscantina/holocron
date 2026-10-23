import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

/** clsx plus tailwind-merge, so `cn('p-2', 'p-4')` renders one padding, not both. */
export const cn = (...inputs: ClassValue[]): string => twMerge(clsx(inputs))

/** Up to two uppercase initials of a display name: "Mike Wallner" gives "MW". */
export const initials = (name: string): string =>
  name
    .split(' ')
    .slice(0, 2)
    .map((part) => part.charAt(0).toUpperCase())
    .join('')
