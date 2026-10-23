<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import type { SetupCheck } from '~/generated'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ check: SetupCheck }>()
const emit = defineEmits<{ continue: [] }>()

const { t } = useI18n()
const { bytes } = useRecordingFormat()

interface Row {
  id: string
  label: string
  value: string
  /** What to change, when the check found a problem. */
  warning?: string
}

const rows = computed<Row[]>(() => {
  const { check } = props
  const origin = window.location.origin
  const publicUrl: Row = {
    id: 'public-url',
    label: t('setup.check.publicUrl'),
    value: check.public_origin ?? t('setup.check.notSet'),
  }
  if (!check.public_origin) publicUrl.warning = t('setup.check.publicUrlMissing', { origin })
  else if (check.public_origin !== origin) publicUrl.warning = t('setup.check.publicUrlOther', { origin })

  const visitor: Row = { id: 'visitor', label: t('setup.check.visitor'), value: check.visitor_address }
  if (check.visitor_address_private) visitor.warning = t('setup.check.visitorPrivate')

  const disk: Row = {
    id: 'disk',
    label: t('setup.check.disk'),
    value: t('setup.check.diskValue', { free: bytes(check.disk_free_bytes), reserve: bytes(check.disk_reserve_bytes) }),
  }
  if (check.disk_free_bytes <= check.disk_reserve_bytes) disk.warning = t('setup.check.diskLow')

  return [publicUrl, visitor, disk, { id: 'version', label: t('setup.check.version'), value: check.version }]
})

const warnings = computed(() => rows.value.filter((row) => row.warning).length)
</script>

<template>
  <form
    class="grid gap-7"
    data-wide
    @submit.prevent="emit('continue')"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.check.title') }}
      <span class="block font-medium text-muted">
        {{ warnings === 0 ? t('setup.check.allGood') : t('setup.check.warnings', { count: warnings }) }}
      </span>
    </h1>
    <ul class="grid gap-2">
      <li
        v-for="row in rows"
        :key="row.id"
        class="flex gap-3 rounded-[var(--radius-control)] bg-fill p-4"
      >
        <Icon
          :name="row.warning ? 'lucide:triangle-alert' : 'lucide:circle-check'"
          class="mt-0.5 shrink-0"
          :class="row.warning ? 'text-warning' : 'text-success'"
          aria-hidden="true"
        />
        <div class="grid min-w-0 gap-0.5">
          <p class="flex flex-wrap gap-x-2 text-sm">
            <span class="font-medium text-primary">{{ row.label }}</span>
            <span class="truncate text-muted">{{ row.value }}</span>
          </p>
          <p
            v-if="row.warning"
            class="text-sm text-muted"
          >
            <span class="sr-only">{{ t('setup.check.warning') }}</span>
            {{ row.warning }}
          </p>
        </div>
      </li>
    </ul>
    <Button
      type="submit"
      variant="accent"
      size="lg"
    >
      {{ t('setup.continue') }}
    </Button>
  </form>
</template>
