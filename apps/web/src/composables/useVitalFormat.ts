import type { Metric } from '~/generated'

import { useExplorationFormat } from './useExplorationFormat'
import { useFormat } from './useFormat'

/** A performance percentile as text: milliseconds with their unit, CLS as the score it is. */
export function useVitalFormat() {
  const { milliseconds } = useFormat()
  const format = useExplorationFormat()
  return (metric: Metric | undefined) => (metric?.t === 'millis' ? milliseconds(metric.v) : format.metric(metric))
}
