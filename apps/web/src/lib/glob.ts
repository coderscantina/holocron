import { GLOB_MAX_BYTES, GLOB_WILDCARDS_MAX } from '~/generated/limits'

export type GlobError = 'empty' | 'too_long' | 'too_many' | 'trailing_escape'

/**
 * Checks a `matches` glob against the server's limits (0022): 1..GLOB_MAX_BYTES bytes, at most
 * GLOB_WILDCARDS_MAX wildcards, no trailing `\`. `*` and `**` each count as one wildcard and `\`
 * escapes the next character.
 */
export function globError(glob: string): GlobError | null {
  if (glob === '') return 'empty'
  if (new TextEncoder().encode(glob).length > GLOB_MAX_BYTES) return 'too_long'
  let wildcards = 0
  for (let i = 0; i < glob.length; i++) {
    const char = glob[i]
    if (char === '\\') {
      if (i === glob.length - 1) return 'trailing_escape'
      i++
    } else if (char === '*') {
      wildcards++
      if (glob[i + 1] === '*') i++
    }
  }
  return wildcards > GLOB_WILDCARDS_MAX ? 'too_many' : null
}
