<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeaderCombined } from '~/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { Range, Retention } from '~/generated'
import { rangePresets, rangePresetsFor } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import { canonical } from '~/services/exploration'

/**
 * Relative presets, and a fixed range of local dates. A relative range stays relative when saved.
 * `chip` renders the trigger as a top-bar scope chip; `until` adds "until now, 15:20" to it.
 * The site's `retention` adds the longer presets and all time; without it (shares, boards) only
 * the presets up to 12 months show.
 */
const props = defineProps<{ range: Range; chip?: boolean; until?: string | null; retention?: Retention }>()
const emit = defineEmits<{ change: [range: Range] }>()

const { t } = useI18n()
const { rangeLabel } = useExplorationFormat()

const presets = computed(() => (props.retention ? rangePresetsFor(props.retention) : rangePresets))

const current = computed(() => canonical(props.range))
const onPreset = (value: unknown) => {
  const preset = presets.value.find((p) => canonical(p.range) === value)
  if (preset) emit('change', preset.range)
}

const custom = ref(false)
const from = ref('')
const to = ref('')
const openCustom = () => {
  if (props.range.kind === 'days') {
    from.value = props.range.start
    to.value = props.range.end
  }
  custom.value = true
}
const applyCustom = () => {
  if (!from.value || !to.value) return
  const [start, end] = from.value <= to.value ? [from.value, to.value] : [to.value, from.value]
  emit('change', { kind: 'days', start, end })
  custom.value = false
}
const fromId = useId()
const toId = useId()
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button
        :variant="chip ? 'fill' : 'outline'"
        :size="chip ? 'control' : 'sm'"
        :aria-label="`${t('explore.range.label')}: ${rangeLabel(range)}${until ? `, ${until}` : ''}`"
      >
        <Icon
          name="lucide:calendar"
          :class="chip && 'text-muted'"
          aria-hidden="true"
        />
        <span class="max-w-48 truncate">{{ rangeLabel(range) }}</span>
        <small
          v-if="until"
          class="text-[0.875rem] font-normal text-muted"
        >
          {{ until }}
        </small>
        <Icon
          v-if="!chip"
          name="lucide:chevron-down"
          class="text-muted"
          aria-hidden="true"
        />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent
      align="start"
      class="w-56"
    >
      <DropdownMenuRadioGroup
        :model-value="current"
        @update:model-value="onPreset"
      >
        <DropdownMenuRadioItem
          v-for="preset in presets"
          :key="preset.key"
          :value="canonical(preset.range)"
        >
          {{ t(preset.key) }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
      <DropdownMenuSeparator />
      <DropdownMenuItem @select="openCustom">
        <Icon
          name="lucide:calendar-range"
          aria-hidden="true"
        />
        {{ t('explore.range.custom') }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>

  <Dialog v-model:open="custom">
    <DialogContent class="sm:max-w-sm">
      <DialogHeaderCombined :title="t('explore.range.custom')" />
      <form
        class="grid gap-3"
        @submit.prevent="applyCustom"
      >
        <div class="grid gap-1.5">
          <Label :for="fromId">{{ t('explore.range.from') }}</Label>
          <Input
            :id="fromId"
            v-model="from"
            type="date"
            required
          />
        </div>
        <div class="grid gap-1.5">
          <Label :for="toId">{{ t('explore.range.to') }}</Label>
          <Input
            :id="toId"
            v-model="to"
            type="date"
            required
          />
        </div>
        <DialogFooter>
          <Button
            type="submit"
            variant="accent"
          >
            {{ t('explore.range.apply') }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
