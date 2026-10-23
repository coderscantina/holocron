<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import ReplayDetail from '~/components/recordings/ReplayDetail.vue'
import { Button } from '~/components/ui/button'
import type { Share } from '~/composables/useSharedView'
import type { SharedView } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import ShareHeader from './ShareHeader.vue'

/**
 * One replay through the share, in the same isolated replay frame as the app. A replay outside
 * the share answers `not_found` and shows as not available.
 */
const props = defineProps<{ share: Share; view: SharedView }>()

const { t } = useI18n()
const route = useRoute()

const token = computed(() => String(route.params.token))
const captureId = computed(() => String(route.params.captureId))
const site = computed(() => {
  const wanted = typeof route.query.site === 'string' ? route.query.site : null
  const target = props.view.target
  const fallback = target.kind === 'exploration' ? target.state.site : props.view.sites[0]?.id
  return props.view.sites.find((entry) => entry.id === wanted)?.id ?? fallback ?? ''
})
</script>

<template>
  <ShareHeader
    :view="view"
    :title="t('replays.player.title')"
  >
    <Button
      as-child
      variant="outline"
      size="sm"
    >
      <RouterLink :to="{ name: 'share-replays', params: { token }, query: { site } }">
        <Icon
          name="lucide:arrow-left"
          aria-hidden="true"
        />
        {{ t('replays.player.back') }}
      </RouterLink>
    </Button>
  </ShareHeader>
  <ReplayDetail
    :key="captureId"
    :site-id="site"
    :capture-id="captureId"
    :share="share.scope"
  />
</template>
