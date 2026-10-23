export const setupPaths = ['fresh', 'import', 'restore'] as const
export type SetupPath = (typeof setupPaths)[number]

const shared = ['token', 'check', 'path'] as const

/** The wizard's steps in order. Each path continues the shared start with its own steps. */
export const setupSteps = {
  fresh: [...shared, 'admin', 'site'],
  import: [...shared, 'admin', 'source', 'options', 'import', 'report'],
  restore: [...shared, 'backup', 'restore'],
} as const satisfies Record<SetupPath, readonly string[]>

export type SetupStep = (typeof setupSteps)[SetupPath][number]
