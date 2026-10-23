<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { SimpleTooltip } from '~/components/ui/tooltip'
import type { ApiClient } from '~/api/client'
import { explorationsResource } from '~/api/resources/explorations'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { ExplorationState, ExportFormat } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * Every row of the state as a file. The server computes the export from the same state, so it
 * carries the same predicates, dates, versions and dimensions as the screen.
 */
const props = defineProps<{
  state: ExplorationState
  /** Reads through this client instead of the session, as share viewers do. */
  client?: ApiClient
}>()
const emit = defineEmits<{ notice: [message: string, tone: 'success' | 'error'] }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const busy = ref(false)

const download = async (format: ExportFormat) => {
  busy.value = true
  try {
    const file = await explorationsResource.export(props.state, format, props.client)
    const url = URL.createObjectURL(new Blob([file.content], { type: file.content_type }))
    const link = document.createElement('a')
    link.href = url
    link.download = file.filename
    link.click()
    URL.revokeObjectURL(url)
    emit('notice', t('explore.export.done', { file: file.filename }), 'success')
  } catch (error) {
    emit('notice', message(error), 'error')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <SimpleTooltip
        :tooltip="t('explore.export.label')"
        as-child
      >
        <Button
          variant="ghost"
          size="icon"
          class="size-7 text-muted hover:text-primary max-sm:size-10"
          :aria-label="t('explore.export.label')"
          :disabled="busy"
        >
          <Icon
            name="lucide:download"
            aria-hidden="true"
          />
        </Button>
      </SimpleTooltip>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      <DropdownMenuItem @select="download('csv')">
        <Icon
          name="lucide:sheet"
          aria-hidden="true"
        />
        {{ t('explore.export.csv') }}
      </DropdownMenuItem>
      <DropdownMenuItem @select="download('json')">
        <Icon
          name="lucide:file-json"
          aria-hidden="true"
        />
        {{ t('explore.export.json') }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
