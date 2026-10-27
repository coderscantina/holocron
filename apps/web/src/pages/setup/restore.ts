import type { SetupRestorePhase, SetupRestoreProgress, SetupStepState } from '~/generated'

export interface RestoreRow {
  phase: SetupRestorePhase
  state: SetupStepState
}

/**
 * The phases a restore shows, in order, as the import's step rows do. Upgrading appears only
 * when the backup is older than this release.
 */
export function restoreRows(progress: SetupRestoreProgress): RestoreRow[] {
  const complete = progress.state === 'complete'
  const upgrading = progress.phase === 'upgrading'
  const rows: RestoreRow[] = [
    {
      phase: 'restoring',
      state: complete || upgrading ? 'done' : progress.state === 'running' ? 'running' : 'pending',
    },
  ]
  if (progress.upgrade) {
    rows.push({ phase: 'upgrading', state: complete ? 'done' : upgrading ? 'running' : 'pending' })
  }
  return rows
}
