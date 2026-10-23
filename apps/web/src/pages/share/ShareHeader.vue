<script setup lang="ts">
import PageHeader from '~/components/PageHeader.vue'
import { Badge } from '~/components/ui/badge'
import { useDateFormat } from '~/composables/useDateFormat'
import type { SharedView } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/** The share's name, sites and end date, with the page's own actions. */
const props = defineProps<{ view: SharedView; title?: string }>()

const { t } = useI18n()
const { date } = useDateFormat()

const sites = computed(() => props.view.sites.map((site) => site.name).join(', '))
</script>

<template>
  <PageHeader
    :title="title ?? view.name"
    :description="title ? `${view.name} · ${sites}` : sites"
  >
    <template #eyebrow>
      <Badge
        variant="surface"
        size="sm"
      >
        {{ t('share.badge') }}
      </Badge>
      <span>{{ t('share.expires', { date: date(view.expires_at) }) }}</span>
    </template>
    <template
      v-if="$slots.default"
      #actions
    >
      <slot />
    </template>
  </PageHeader>
</template>
