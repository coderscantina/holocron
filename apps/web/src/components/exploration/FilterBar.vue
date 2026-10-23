<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/ui/data-table/TableFilter.vue. The
// flat `field -> operator:value` bag is replaced by the exploration's predicate tree: fields can
// repeat, conditions keep their IDs, and groups join with AND or OR.
import { useQuery } from '@tanstack/vue-query'
import { useMediaQuery } from '@vueuse/core'

import CountryFlag from '~/components/CountryFlag.vue'
import Icon from '~/components/Icon.vue'
import { SplitBadge } from '~/components/ui/badge'
import { Popover, PopoverContent, PopoverTrigger } from '~/components/ui/popover'
import { queryKeys } from '~/api/query-keys'
import { explorationsResource } from '~/api/resources/explorations'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import {
  kindOf,
  operatorsFor,
  parseValue,
  valueless,
  valueText,
  type FilterField,
} from '~/composables/useFilterFields'
import type { Condition, ExplorationState, Operator } from '~/generated'
import { conditionCountry, countryCode } from '~/lib/client-looks'
import { operatorLabels, typeLabels } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import {
  addCondition,
  canonical,
  clearFilters,
  conditions,
  groupWithPrevious,
  otherOp,
  removeCondition,
  replaceCondition,
  filterTree,
  setGroupOp,
  treeAt,
  ungroup,
  type TreePath,
} from '~/services/exploration'

import FilterTree from './FilterTree.vue'

/**
 * One input that builds conditions in stages: field, then how it matches, then the value.
 * Each stage is a list to arrow through; Backspace walks a stage back and, on an empty input,
 * removes the last condition; Escape abandons the half-built one. Every committed change is
 * one new state, so Back undoes it. Below `sm` the chips collapse into a count that opens a
 * popover, keeping the input wide enough to type in.
 */
const props = defineProps<{
  state: ExplorationState
  fields: FilterField[]
  /** Server refusals by predicate ID. */
  refused?: Record<string, string>
  /** The tree it edits: the filter, or a report's step or goal. */
  tree?: TreePath
  /** Accessible name of the input when it edits a report tree. */
  label?: string
}>()

const emit = defineEmits<{
  commit: [state: ExplorationState]
}>()

const { t } = useI18n()
const format = useExplorationFormat()

const instanceId = useId()
const listboxId = `${instanceId}-listbox`

type Stage = 'field' | 'operator' | 'value'

const input = ref('')
const stage = ref<Stage>('field')
const open = ref(false)
const highlighted = ref(0)
const pendingField = ref<FilterField | null>(null)
const pendingOperator = ref<Operator | null>(null)
const editing = ref<string | null>(null)
const announcement = ref('')
const invalid = ref<string | null>(null)

const chipsOpen = ref(false)
const isCompact = useMediaQuery('(width < 40rem)')

const container = ref<HTMLElement | null>(null)
const inputEl = ref<HTMLInputElement | null>(null)
const listEl = ref<HTMLElement | null>(null)

const tree = computed(() => props.tree ?? filterTree)
const root = computed(() => treeAt(props.state, tree.value))
const count = computed(() => conditions(root.value).length)

const fieldLabel = (condition: Condition) => format.fieldLabel(condition.field)
/** The operator and value in words; a named country is left to the chip's flag and name. */
const summary = (condition: Condition) =>
  [t(operatorLabels[condition.cmp]), condition.value && !conditionCountry(condition) ? valueText(condition.value) : '']
    .filter(Boolean)
    .join(' ')

const suggestionField = computed(() =>
  stage.value === 'value' && pendingField.value && ['text', 'string'].includes(pendingField.value.kind)
    ? pendingField.value.field
    : null
)
const suggestions = useQuery({
  queryKey: computed(() => queryKeys.values(props.state.site, canonical(suggestionField.value))),
  queryFn: ({ signal }) => {
    const field = suggestionField.value
    if (!field) throw new Error('no field')
    return explorationsResource.values(props.state.site, field, signal)
  },
  enabled: computed(() => suggestionField.value !== null),
  staleTime: 5 * 60 * 1000,
})

const matches = (label: string) => input.value === '' || label.toLowerCase().includes(input.value.toLowerCase())

interface Item {
  key: string
  label: string
  hint?: string
  flag?: string | null
}

const items = computed<Item[]>(() => {
  if (stage.value === 'field') {
    return props.fields
      .filter((field) => matches(field.label))
      .map((field) => ({ key: field.key, label: field.label }))
  }
  const field = pendingField.value
  if (!field) return []
  if (stage.value === 'operator') {
    return operatorsFor[field.kind]
      .map((operator) => ({ key: operator, label: t(operatorLabels[operator]) }))
      .filter((item) => matches(item.label))
  }
  if (field.kind === 'bool') {
    return [
      { key: 'true', label: t('explore.filters.true') },
      { key: 'false', label: t('explore.filters.false') },
    ]
  }
  return (suggestions.data.value?.values ?? [])
    .flatMap((value) => {
      if (value.value.t !== 'string') return []
      const flag = countryCode(field.field, value.value)
      const label = flag ? format.textLabel('country', flag) : value.value.v
      return matches(label) || matches(value.value.v)
        ? [{ key: value.value.v, label, hint: String(value.events), flag }]
        : []
    })
})

watch(items, () => {
  highlighted.value = 0
})

const placeholder = computed(() => {
  const label = pendingField.value?.label ?? ''
  if (stage.value === 'operator') return t('explore.filters.pickOperator', { field: label })
  if (stage.value === 'value') {
    return ['text', 'string', 'bool'].includes(pendingField.value?.kind ?? '')
      ? t('explore.filters.pickValue', { field: label })
      : t('explore.filters.enterValue', { field: label })
  }
  return t('explore.filters.placeholder')
})

const stageLabel = computed(() =>
  t(
    stage.value === 'field'
      ? 'explore.filters.stageField'
      : stage.value === 'operator'
        ? 'explore.filters.stageOperator'
        : 'explore.filters.stageValue'
  )
)

const announce = (message: string) => {
  announcement.value = ''
  void nextTick(() => {
    announcement.value = message
  })
}

const focusInput = () => void nextTick(() => inputEl.value?.focus())

const resetPending = () => {
  pendingField.value = null
  pendingOperator.value = null
  editing.value = null
  stage.value = 'field'
  input.value = ''
  invalid.value = null
  highlighted.value = 0
}

const apply = (condition: Omit<Condition, 'id' | 'kind'>) => {
  const next = editing.value
    ? replaceCondition(props.state, editing.value, condition, tree.value)
    : addCondition(props.state, condition, tree.value)
  emit('commit', next)
  announce(t('explore.filters.applied', { field: format.fieldLabel(condition.field) }))
  resetPending()
  open.value = false
}

const chooseField = (field: FilterField) => {
  pendingField.value = field
  pendingOperator.value = null
  input.value = ''
  open.value = true
  stage.value = 'operator'
  announce(t('explore.filters.fieldPicked', { field: field.label }))
  focusInput()
}

const chooseOperator = (operator: Operator) => {
  const field = pendingField.value
  if (!field) return
  pendingOperator.value = operator
  input.value = ''
  if (valueless.includes(operator)) {
    apply({ field: field.field, cmp: operator })
    return
  }
  stage.value = 'value'
  open.value = true
  focusInput()
}

const chooseValue = (text?: string) => {
  const field = pendingField.value
  const operator = pendingOperator.value
  if (!field || !operator) return
  const value = parseValue(field.kind, text ?? input.value)
  if (!value) {
    // Keep the half-built condition and say what is missing.
    const type = field.kind === 'number' ? 'decimal' : field.kind === 'date' ? 'date' : 'integer'
    invalid.value = t('explore.filters.invalidValue', { type: t(typeLabels[type]) })
    return
  }
  apply({ field: field.field, cmp: operator, value })
}

const editCondition = (condition: Condition) => {
  const kind = kindOf(condition)
  const field =
    props.fields.find((f) => canonical(f.field) === canonical(condition.field) && f.kind === kind) ??
    ({ key: canonical(condition.field), field: condition.field, label: fieldLabel(condition), kind } satisfies FilterField)
  editing.value = condition.id
  chipsOpen.value = false
  pendingField.value = field
  pendingOperator.value = condition.cmp
  if (valueless.includes(condition.cmp)) {
    stage.value = 'operator'
    input.value = ''
  } else {
    stage.value = 'value'
    input.value = condition.value ? valueText(condition.value) : ''
  }
  open.value = true
  announce(t('explore.filters.editing', { field: field.label }))
  focusInput()
}

const remove = (condition: Condition) => {
  emit('commit', removeCondition(props.state, condition.id, tree.value))
  announce(t('explore.filters.removed', { field: fieldLabel(condition) }))
  focusInput()
}

const clearAll = () => {
  resetPending()
  open.value = false
  emit('commit', clearFilters(props.state, tree.value))
  announce(t('explore.filters.cleared'))
  focusInput()
}

const toggleRoot = () => {
  if (root.value) emit('commit', setGroupOp(props.state, [], otherOp(root.value.op), tree.value))
}

const settle = () => {
  if (stage.value === 'value' && input.value.trim() !== '' && !invalid.value) chooseValue()
  else if (!invalid.value) resetPending()
  open.value = false
}

const pick = (index: number) => {
  const item = items.value[index]
  if (!item) return
  if (stage.value === 'field') {
    const field = props.fields.find((f) => f.key === item.key)
    if (field) chooseField(field)
    return
  }
  if (stage.value === 'operator') {
    const operator = operatorsFor[pendingField.value?.kind ?? 'text'].find((op) => op === item.key)
    if (operator) chooseOperator(operator)
    return
  }
  chooseValue(item.key)
}

const scrollToHighlighted = () => {
  void nextTick(() => {
    listEl.value?.querySelector(`#${listboxId}-item-${highlighted.value}`)?.scrollIntoView({ block: 'nearest' })
  })
}

const stepBack = () => {
  invalid.value = null
  if (stage.value === 'value') {
    stage.value = 'operator'
    pendingOperator.value = null
    return
  }
  stage.value = 'field'
  pendingField.value = null
}

const onKeydown = (event: KeyboardEvent) => {
  if (open.value && items.value.length > 0) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      highlighted.value = (highlighted.value + 1) % items.value.length
      scrollToHighlighted()
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      highlighted.value = highlighted.value <= 0 ? items.value.length - 1 : highlighted.value - 1
      scrollToHighlighted()
      return
    }
  }
  if (event.key === 'Enter') {
    event.preventDefault()
    // Substring operators take the typed text; equality picks the highlighted suggestion.
    const partial = ['contains', 'not_contains', 'starts_with'].includes(pendingOperator.value ?? '')
    if (stage.value === 'value' && partial && input.value.trim() !== '') chooseValue()
    else if (open.value && items.value.length > 0) pick(highlighted.value)
    else if (stage.value === 'value') chooseValue()
    return
  }
  if (event.key === 'Backspace' && input.value === '') {
    if (stage.value !== 'field') {
      event.preventDefault()
      stepBack()
      open.value = true
      announce(t('explore.filters.wentBack'))
      return
    }
    const last = conditions(root.value).at(-1)
    if (last) {
      event.preventDefault()
      remove(last)
    }
    return
  }
  if (event.key === 'Escape') {
    if (stage.value !== 'field' || editing.value) {
      event.preventDefault()
      resetPending()
    } else if (open.value) {
      event.preventDefault()
      open.value = false
    } else if (input.value !== '') {
      event.preventDefault()
      input.value = ''
    }
    return
  }
  if (event.key === 'Tab') settle()
}

const onClickOutside = (event: MouseEvent) => {
  if (chipsOpen.value) return
  const target = event.target
  if (container.value && target instanceof Node && !container.value.contains(target)) settle()
}

onMounted(() => document.addEventListener('mousedown', onClickOutside))
onBeforeUnmount(() => document.removeEventListener('mousedown', onClickOutside))

const onDate = (event: Event) => {
  if (event.target instanceof HTMLInputElement) chooseValue(event.target.value)
}

const refusedById = computed(() => props.refused ?? {})
const hasAnything = computed(() => count.value > 0 || input.value !== '')
const showDateInput = computed(() => stage.value === 'value' && pendingField.value?.kind === 'date')
</script>

<template>
  <div
    ref="container"
    class="relative w-full min-w-0"
  >
    <div
      class="flex min-h-8 w-full flex-wrap items-center gap-1.5 rounded-md border border-border bg-transparent py-0.5 pr-1.5 pl-2.5 text-sm transition-colors hover:border-input-border focus-within:border-ring max-sm:min-h-10"
      :class="invalid ? 'border-destructive' : ''"
    >
      <Icon
        name="lucide:list-filter"
        size="15"
        class="shrink-0 text-muted"
        aria-hidden="true"
      />

      <Popover
        v-if="isCompact && root && count > 0"
        v-model:open="chipsOpen"
      >
        <PopoverTrigger
          class="flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded-[5px] bg-secondary px-1.5 text-xs font-medium text-secondary-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          :aria-label="t('explore.filters.chipCount', { count })"
        >
          <Icon
            name="lucide:list-filter"
            size="13"
            aria-hidden="true"
          />
          {{ count }}
        </PopoverTrigger>
        <PopoverContent
          align="start"
          class="w-[min(22rem,calc(100vw-2*var(--shell-gutter-x)))] p-2"
        >
          <div class="flex flex-wrap items-center gap-1.5">
            <FilterTree
              :root="root"
              :field-label="fieldLabel"
              :summary="summary"
              :refused="refusedById"
              :editing="editing"
              @edit="editCondition"
              @remove="remove"
              @group="(index) => emit('commit', groupWithPrevious(props.state, index, tree))"
              @ungroup="(index) => emit('commit', ungroup(props.state, index, tree))"
              @toggle="toggleRoot"
            />
          </div>
        </PopoverContent>
      </Popover>

      <FilterTree
        v-else-if="root"
        :root="root"
        :field-label="fieldLabel"
        :summary="summary"
        :refused="refusedById"
        :editing="editing"
        @edit="editCondition"
        @remove="remove"
        @group="(index) => emit('commit', groupWithPrevious(props.state, index, tree))"
        @ungroup="(index) => emit('commit', ungroup(props.state, index, tree))"
        @toggle="toggleRoot"
      />

      <SplitBadge
        v-if="pendingField"
        size="sm"
        variant="secondary"
        label-variant="surface"
        :label="pendingField.label"
      >
        {{ pendingOperator ? t(operatorLabels[pendingOperator]) : '…' }}
      </SplitBadge>

      <input
        ref="inputEl"
        v-model="input"
        type="text"
        role="combobox"
        :data-shortcut-search="tree.kind === 'filter' ? '' : undefined"
        class="min-w-28 flex-1 border-none bg-transparent py-1 text-sm text-foreground placeholder:text-muted focus:outline-none"
        :placeholder="placeholder"
        :aria-label="label && stage === 'field' ? label : placeholder"
        :aria-expanded="open"
        aria-haspopup="listbox"
        :aria-invalid="invalid ? 'true' : undefined"
        :aria-controls="open ? listboxId : undefined"
        :aria-activedescendant="open && items.length > 0 ? `${listboxId}-item-${highlighted}` : undefined"
        @focus="open = true"
        @input="(open = true), (invalid = null)"
        @keydown="onKeydown"
      />

      <button
        v-if="hasAnything"
        type="button"
        class="flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-full text-muted transition-colors hover:bg-secondary hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring max-sm:size-8"
        :aria-label="t('explore.filters.clearAll')"
        @click="clearAll"
      >
        <Icon
          name="lucide:x"
          size="14"
        />
      </button>
    </div>

    <p
      v-if="invalid"
      class="mt-1 text-xs text-destructive"
      role="alert"
    >
      {{ invalid }}
    </p>

    <div
      v-if="open && (items.length > 0 || stage !== 'field' || input !== '')"
      :id="listboxId"
      ref="listEl"
      role="listbox"
      :aria-label="stageLabel"
      class="absolute z-50 mt-1.5 max-h-[min(16rem,50vh)] w-full overflow-y-auto rounded-lg border border-popover-border bg-popover p-1 shadow-soft-lg"
    >
      <template v-if="items.length > 0">
        <div
          v-for="(item, index) in items"
          :id="`${listboxId}-item-${index}`"
          :key="item.key"
          role="option"
          :aria-selected="index === highlighted"
          class="flex cursor-pointer items-center justify-between gap-2 rounded-md px-2 py-1.5 text-sm text-popover-foreground select-none aria-selected:bg-accent aria-selected:text-accent-foreground max-sm:min-h-10"
          @mousemove="highlighted = index"
          @mousedown.prevent
          @click="pick(index)"
        >
          <span class="flex min-w-0 items-center gap-2">
            <CountryFlag
              v-if="item.flag"
              :code="item.flag"
            />
            <span class="truncate">{{ item.label }}</span>
          </span>
          <span
            v-if="item.hint"
            class="text-xs tabular-nums opacity-70"
          >
            {{ item.hint }}
          </span>
        </div>
      </template>
      <div
        v-else-if="showDateInput"
        class="p-2"
      >
        <label
          :for="`${listboxId}-date`"
          class="sr-only"
        >
          {{ placeholder }}
        </label>
        <input
          :id="`${listboxId}-date`"
          type="date"
          class="w-full rounded-md border border-input-border bg-input px-2 py-1.5 text-sm text-foreground focus-visible:border-ring focus-visible:outline-none"
          @change="onDate"
        />
      </div>
      <p
        v-else-if="stage === 'value'"
        class="px-2 py-2 text-sm text-muted select-none"
        role="status"
      >
        {{ t('explore.filters.typeValue') }}
      </p>
      <p
        v-else
        class="px-2 py-2 text-sm text-muted select-none"
        role="status"
      >
        {{ t('explore.filters.noMatch') }}
      </p>
    </div>

    <span
      class="sr-only"
      role="status"
      aria-live="polite"
      aria-atomic="true"
    >
      {{ announcement }}
    </span>
  </div>
</template>
