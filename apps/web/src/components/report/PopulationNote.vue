<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '~/components/ui/tooltip'
import { useFormat } from '~/composables/useFormat'
import type { Coverage } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * Who a behavioral report counts, behind an info icon for a panel header: daily scope stops at
 * local midnight, person scope is the consented population with its share of events, and events
 * without an identity are named rather than silently left out (0003 Common terms). The text is
 * also the icon's accessible description, so it does not depend on the tooltip opening.
 */
const props = defineProps<{
  notes: { scope: 'daily' | 'person' | 'session'; coverage: Coverage }[]
}>()

const { t } = useI18n()
const format = useFormat()
const id = useId()

const lines = computed(() =>
  props.notes.flatMap(({ scope, coverage }) => {
    const all = format.integer(coverage.eligible_events)
    const missing = coverage.eligible_events - coverage.with_identity
    if (scope === 'person') {
      const share = coverage.eligible_events > 0 ? format.percent(coverage.with_identity / coverage.eligible_events) : '–'
      return [t('reports.population.person', { share, linked: format.integer(coverage.with_identity), all })]
    }
    return [
      ...(scope === 'daily' ? [t('reports.population.daily')] : []),
      ...(missing > 0 ? [t('reports.population.missing', { missing: format.integer(missing), all })] : []),
    ]
  })
)
</script>

<template>
  <TooltipProvider
    v-if="lines.length > 0"
    :delay-duration="200"
  >
    <Tooltip>
      <TooltipTrigger as-child>
        <Button
          variant="ghost"
          size="toolbar"
          class="text-muted hover:text-primary"
          :aria-label="t('reports.population.label')"
          :aria-describedby="id"
        >
          <Icon
            name="lucide:info"
            aria-hidden="true"
          />
        </Button>
      </TooltipTrigger>
      <TooltipContent class="grid max-w-xs gap-1.5 py-2 leading-relaxed">
        <p
          v-for="line in lines"
          :key="line"
        >
          {{ line }}
        </p>
      </TooltipContent>
    </Tooltip>
  </TooltipProvider>
  <span
    v-if="lines.length > 0"
    :id="id"
    class="sr-only"
  >
    {{ lines.join(' ') }}
  </span>
</template>
