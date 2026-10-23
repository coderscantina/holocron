import type { Group } from '~/generated'
import { operatorLabels } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import { valueText } from './useFilterFields'

import { useExplorationFormat } from './useExplorationFormat'

/** A predicate tree as one line of text, for goals and steps without a saved name. */
export function usePredicateText() {
  const { t } = useI18n()
  const { fieldLabel } = useExplorationFormat()

  const text = (group: Group): string =>
    group.items
      .map((item) =>
        item.kind === 'group'
          ? `(${text(item)})`
          : [fieldLabel(item.field), t(operatorLabels[item.cmp]), item.value ? valueText(item.value) : '']
              .filter(Boolean)
              .join(' ')
      )
      .join(` ${t(group.op === 'and' ? 'explore.filters.and' : 'explore.filters.or')} `)

  return { text }
}
