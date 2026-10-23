<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import SiteBoundary from '~/components/SiteBoundary.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import { useReplay, useSaveReplay } from '~/composables/useRecordings'
import { useI18n } from '~/plugins/i18n'

import ReplayDetail from '~/components/recordings/ReplayDetail.vue'

const route = useRoute()
const { t } = useI18n()
const recordingFormat = useRecordingFormat()
const { message } = useErrorMessage()

const siteId = computed(() => String(route.params.siteId))
const captureId = computed(() => String(route.params.captureId))
usePageMeta(() => t('nav.replays'))

const replay = useReplay(siteId, captureId)
const save = useSaveReplay()
const capture = computed(() => replay.data.value?.pages[0]?.capture ?? null)

const toggleSaved = () => {
  if (!capture.value) return
  save.mutate({ args: { site: siteId.value, capture: captureId.value }, save: !capture.value.saved })
}
</script>

<template>
  <SiteBoundary :site-id="siteId">
    <PageHeader
      :title="t('replays.player.title')"
      :description="capture ? recordingFormat.dateTime(capture.received_at) : undefined"
    >
      <template #eyebrow>
        <RouterLink
          :to="{ name: 'replays', params: { siteId } }"
          class="inline-flex items-center gap-1 hover:text-primary hover:underline"
        >
          <Icon
            name="lucide:arrow-left"
            size="12"
            aria-hidden="true"
          />
          {{ t('replays.player.back') }}
        </RouterLink>
      </template>
      <template
        v-if="capture"
        #actions
      >
        <Button
          variant="fill"
          class="text-sm"
          :loading="save.isPending.value"
          :aria-pressed="capture.saved"
          @click="toggleSaved"
        >
          <Icon
            :name="capture.saved ? 'lucide:bookmark-check' : 'lucide:bookmark'"
            aria-hidden="true"
          />
          {{ capture.saved ? t('replays.player.unsave') : t('replays.player.save') }}
        </Button>
      </template>
    </PageHeader>

    <Alert
      v-if="save.error.value"
      color="destructive"
      class="mb-4"
    >
      <p class="text-sm">{{ message(save.error.value) }}</p>
    </Alert>

    <ReplayDetail
      :site-id="siteId"
      :capture-id="captureId"
      :session-link="(session) => ({ name: 'replays', params: { siteId }, query: { session: String(session), range: '14d' } })"
    />
  </SiteBoundary>
</template>
