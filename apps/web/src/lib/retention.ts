import type { Retention } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/** How long analytical events are kept (decision 0016), shortest first. */
export const retentions = ['1y', '2y', '3y', '5y', '10y', 'forever'] as const satisfies readonly Retention[]

export const retentionLabels = {
  '1y': 'dataRetention.values.1y',
  '2y': 'dataRetention.values.2y',
  '3y': 'dataRetention.values.3y',
  '5y': 'dataRetention.values.5y',
  '10y': 'dataRetention.values.10y',
  forever: 'dataRetention.values.forever',
} as const satisfies Record<Retention, MessageKey>

const months = { '1y': 12, '2y': 24, '3y': 36, '5y': 60, '10y': 120, forever: Infinity } as const satisfies Record<
  Retention,
  number
>

/** Whether `next` keeps events for less time than `current`, so saving it removes stored events. */
export const isShorter = (next: Retention, current: Retention) => months[next] < months[current]

/** Whether a relative range of `count` months stays inside the retention. */
export const coversMonths = (retention: Retention, count: number) => months[retention] >= count
