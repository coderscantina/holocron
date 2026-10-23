<script setup lang="ts" generic="T extends PaletteResult">
import Icon from '~/components/Icon.vue'
import { CommandGroup, CommandItem } from '~/components/ui/command'
import { useCommand } from '~/components/ui/command/context'
import { visibleResults, type PaletteResult } from '~/composables/useCommandPalette'
import { useI18n } from '~/plugins/i18n'

/** One capped result group of the palette; the rest of it opens with its last row. */
const props = defineProps<{ heading: string; icon: string; items: T[] }>()
const emit = defineEmits<{ select: [item: T] }>()

const { t } = useI18n()
const { filterState } = useCommand()
const expanded = ref(false)
const results = computed(() => visibleResults(props.items, filterState.search, expanded.value))

const showAll = () => {
  const search = filterState.search
  expanded.value = true
  void nextTick(() => {
    filterState.search = search
  })
}
</script>

<template>
  <CommandGroup
    v-if="results.total > 0"
    :heading="heading"
  >
    <CommandItem
      v-for="item in results.shown"
      :key="item.id"
      :value="item.label"
      @select="emit('select', item)"
    >
      <Icon :name="icon" />
      <span class="min-w-0 flex-1 truncate">{{ item.label }}</span>
      <slot
        name="hint"
        :item="item"
      />
    </CommandItem>
    <CommandItem
      v-if="results.total > results.shown.length"
      :key="`all-${filterState.search}`"
      :value="`${heading} ${filterState.search}`"
      class="text-muted"
      @select="showAll"
    >
      <Icon name="lucide:list" />
      {{ t('commandPalette.showAll', { count: results.total }) }}
      <span class="sr-only">{{ filterState.search }}</span>
    </CommandItem>
  </CommandGroup>
</template>
