<script setup lang="ts">
import AnnotationsControl from '~/components/exploration/AnnotationsControl.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { SimpleTooltip } from '~/components/ui/tooltip'
import type { Finding, Group, Site } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { encodeFilter } from '~/services/exploration'

import { filterOf, findingActions, reportLocation, type FindingAction } from './actions'
import type { FindingText } from './text'
import { useFindingReplays } from './useFindingReplays'

/**
 * The buttons of one finding, all operations on its own state. The first is filled, the rest
 * are ghost, and each is named after the finding it belongs to.
 */
const props = defineProps<{ site: Site; finding: Finding; text: FindingText }>()
const emit = defineEmits<{ filter: [filter: Group] }>()

const DAY_MS = 86_400_000

const { t } = useI18n()
const replays = useFindingReplays(
  () => props.site,
  () => filterOf(props.finding, props.text.known)
)
const actions = computed(() =>
  findingActions(props.finding, props.text.known, {
    replays: replays.value,
    canAnnotate: props.site.capabilities.includes('site.definitions.write'),
  })
)

const label = (action: FindingAction) => {
  const name =
    action.id === 'replays'
      ? t('findings.action.replays', { count: action.count })
      : t(`findings.action.${action.id}`)
  return { name, full: t('findings.action.label', { action: name, fact: props.text.fact }) }
}
const variant = (index: number) => (index === 0 ? 'fill' : 'ghost')
</script>

<template>
  <div class="flex flex-wrap items-center gap-1.5">
    <template
      v-for="(action, index) in actions"
      :key="action.id"
    >
      <Button
        v-if="action.id === 'filter'"
        :variant="variant(index)"
        :aria-label="label(action).full"
        @click="emit('filter', action.filter)"
      >
        {{ label(action).name }}
      </Button>
      <Button
        v-else-if="action.id === 'report' || action.id === 'details'"
        as-child
        :variant="variant(index)"
      >
        <RouterLink
          :to="reportLocation(site.id, action.route, finding)"
          :aria-label="label(action).full"
        >
          {{ label(action).name }}
        </RouterLink>
      </Button>
      <Button
        v-else-if="action.id === 'replays'"
        as-child
        :variant="variant(index)"
      >
        <RouterLink
          :to="{ name: 'replays', params: { siteId: site.id }, query: { range: '1d', filter: encodeFilter(action.filter) } }"
          :aria-label="label(action).full"
        >
          <Icon
            name="lucide:circle-play"
            aria-hidden="true"
          />
          {{ label(action).name }}
        </RouterLink>
      </Button>
      <AnnotationsControl
        v-else
        :site="site.id"
        :range="{ start: finding.window_start, end: finding.window_start + DAY_MS }"
        :timezone="finding.state.timezone"
        :at="finding.window_start"
        can-edit
      >
        <template #trigger>
          <SimpleTooltip
            :tooltip="label(action).name"
            as-child
          >
            <Button
              :variant="variant(index)"
              size="icon"
              class="text-muted hover:text-primary"
              :aria-label="label(action).full"
            >
              <Icon
                name="lucide:message-square-plus"
                aria-hidden="true"
              />
            </Button>
          </SimpleTooltip>
        </template>
      </AnnotationsControl>
    </template>
  </div>
</template>
