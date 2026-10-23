import { useRecordingFormat } from '~/composables/useRecordingFormat'
import type { DiskUsage, MaintenanceState, MaintenanceTask, ReadinessProblem, StorageForecast } from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/** Problems that stop collection (503); the others leave it ready. */
const blocking = new Set<ReadinessProblem>(['disk_full', 'storage_unavailable', 'shutting_down'])

export const blocksCollection = (problem: ReadinessProblem) => blocking.has(problem)

export const problemTitles = {
  disk_full: 'admin.status.problems.diskFull.title',
  storage_unavailable: 'admin.status.problems.storageUnavailable.title',
  shutting_down: 'admin.status.problems.shuttingDown.title',
  transfer_failing: 'admin.status.problems.transferFailing.title',
  analytics_stale: 'admin.status.problems.analyticsStale.title',
  recorder_budget: 'admin.status.problems.recorderBudget.title',
} as const satisfies Record<ReadinessProblem, MessageKey>

export const problemDescriptions = {
  disk_full: 'admin.status.problems.diskFull.description',
  storage_unavailable: 'admin.status.problems.storageUnavailable.description',
  shutting_down: 'admin.status.problems.shuttingDown.description',
  transfer_failing: 'admin.status.problems.transferFailing.description',
  analytics_stale: 'admin.status.problems.analyticsStale.description',
  recorder_budget: 'admin.status.problems.recorderBudget.description',
} as const satisfies Record<ReadinessProblem, MessageKey>

export const taskLabels = {
  retention: 'admin.status.maintenance.retention',
  compaction: 'admin.status.maintenance.compaction',
  backup: 'admin.status.maintenance.backup',
} as const satisfies Record<MaintenanceTask, MessageKey>

export const stateLabels = {
  queued: 'admin.status.maintenance.queued',
  running: 'admin.status.maintenance.running',
  done: 'admin.status.maintenance.done',
  failed: 'admin.status.maintenance.failed',
} as const satisfies Record<MaintenanceState, MessageKey>

/** Everything Holocron's files take. */
export const usedBytes = (disk: DiskUsage) =>
  disk.control_bytes + disk.inbox_bytes + disk.recordings_bytes + disk.analytics_bytes + disk.spill_bytes

/** A share in percent for a meter; 0 when there is nothing to share. */
export const share = (part: number, whole: number) => (whole > 0 ? (part / whole) * 100 : 0)

/** `YYYY-MM-DD` as a UTC instant, so the formatted date keeps its digits. */
export const dayInstant = (day: string) => Date.parse(`${day}T00:00:00Z`)

/** The server state and the storage forecast in words, in the active locale. */
export function useOperatorStatusText() {
  const { t, locale } = useI18n()
  const { bytes } = useRecordingFormat()
  const day = (value: string) =>
    new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeZone: 'UTC' }).format(dayInstant(value))

  return {
    bytes,
    day,
    /** "Healthy", or the first problem `/health/ready` names. */
    state: (problems: readonly ReadinessProblem[]) => {
      const first = problems[0]
      return first ? t(problemTitles[first]) : t('admin.status.healthy')
    },
    /** The forecast's outcome as one sentence. */
    forecast: (forecast: StorageForecast | null) => {
      if (!forecast) return t('admin.status.forecast.noSample')
      const outcome = forecast.outcome
      switch (outcome.kind) {
        case 'levels_off':
          return t('admin.status.forecast.levelsOff', { size: bytes(outcome.size_bytes), date: day(outcome.on) })
        case 'reaches_reserve':
          return t('admin.status.forecast.reachesReserve', { date: day(outcome.on) })
        case 'not_enough_history':
          return t('admin.status.forecast.notEnoughHistory', { days: outcome.days })
      }
    },
  }
}
