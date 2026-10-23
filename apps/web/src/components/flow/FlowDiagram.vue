<script setup lang="ts">
import { onKeyStroke, useElementSize } from '@vueuse/core'

import { bandPath, layoutFlow, type FlowDiagramLink, type FlowDiagramNode } from './layout'

/**
 * A flow diagram filling its parent: nodes as bars in columns, links as bands between them,
 * quiet by default with the emphasised part of each band in the accent. Every node is a
 * button: choosing it selects it, choosing it again or pressing Esc clears the selection.
 * What a selection emphasises is the parent's call, through the links' `emphasis`. Below
 * `minColumnWidth` per column the diagram scrolls sideways instead of squeezing labels.
 */
const props = withDefaults(
  defineProps<{
    nodes: FlowDiagramNode[]
    links: FlowDiagramLink[]
    /** The diagram's accessible name. */
    label: string
    selected?: string | null
    minColumnWidth?: number
  }>(),
  { selected: null, minColumnWidth: 150 }
)

const emit = defineEmits<{ select: [id: string | null] }>()

const NODE_WIDTH = 6
const GAP = 14
/** Room right of the last column for its labels. */
const LABEL_ROOM = 184
/** A label needs this much height, its gap included, to show beside its bar; smaller nodes name themselves on hover and focus. */
const LABEL_HEIGHT = 20

const frame = useTemplateRef('frame')
const size = useElementSize(frame)
const columns = computed(() => Math.max(1, ...props.nodes.map((node) => node.column + 1)))
const width = computed(() => Math.max(size.width.value, (columns.value - 1) * props.minColumnWidth + LABEL_ROOM))
const room = computed(() => Math.min(LABEL_ROOM, Math.max(width.value * 0.28, 176)))

const layout = computed(() =>
  layoutFlow(props.nodes, props.links, {
    width: Math.max(0, width.value - room.value),
    height: Math.max(0, size.height.value - 2),
    nodeWidth: NODE_WIDTH,
    gap: GAP,
  })
)
const step = computed(() => (columns.value > 1 ? (width.value - room.value - NODE_WIDTH) / (columns.value - 1) : width.value))
const labelWidth = (column: number) => (column === columns.value - 1 ? room.value : step.value) - NODE_WIDTH - 10

/** Nodes some emphasis passes through, or the selected one. */
const lit = computed(() => {
  const ids = new Set<string>(props.selected ? [props.selected] : [])
  for (const link of props.links) {
    if ((link.emphasis ?? 0) <= 0) continue
    ids.add(link.source)
    ids.add(link.target)
  }
  return ids
})
const dimmed = computed(() => props.selected !== null)

const bands = computed(() =>
  layout.value.links.map((placed) => ({
    key: `${placed.link.source}>${placed.link.target}`,
    target: placed.link.target,
    path: bandPath(placed.x0, placed.y0, placed.x1, placed.y1, Math.max(0.5, placed.width)),
    emphasis:
      (placed.link.emphasis ?? 0) > 0
        ? bandPath(placed.x0, placed.y0, placed.x1, placed.y1, Math.max(1, (placed.link.emphasis ?? 0) * layout.value.scale))
        : null,
  }))
)

const toggle = (id: string) => emit('select', props.selected === id ? null : id)
onKeyStroke('Escape', () => {
  if (props.selected !== null) emit('select', null)
})
</script>

<template>
  <div
    ref="frame"
    class="relative h-full w-full overflow-x-auto overflow-y-hidden"
  >
    <div
      role="group"
      :aria-label="label"
      class="relative h-full"
      :style="{ width: `${width}px` }"
    >
      <svg
        class="absolute inset-0 overflow-visible"
        :width="width"
        :height="size.height.value"
        aria-hidden="true"
      >
        <g>
          <path
            v-for="band in bands"
            :key="band.key"
            :d="band.path"
            class="cursor-pointer fill-series-secondary motion-safe:transition-opacity motion-safe:duration-200"
            :class="dimmed ? 'opacity-20' : 'opacity-45 hover:opacity-70'"
            @click="toggle(band.target)"
          />
        </g>
        <g class="pointer-events-none">
          <template
            v-for="band in bands"
            :key="band.key"
          >
            <path
              v-if="band.emphasis"
              :d="band.emphasis"
              class="fill-accent opacity-55"
            />
          </template>
        </g>
        <rect
          v-for="placed in layout.nodes"
          :key="placed.node.id"
          :x="placed.x"
          :y="placed.y"
          :width="NODE_WIDTH"
          :height="Math.max(1, placed.height)"
          rx="1"
          class="motion-safe:transition-opacity motion-safe:duration-200"
          :class="[
            placed.node.id === selected || (placed.node.tone === 'accent' && !dimmed) ? 'fill-accent' : placed.node.tone === 'quiet' ? 'fill-series-comparison' : 'fill-primary',
            dimmed && !lit.has(placed.node.id) ? 'opacity-30' : '',
          ]"
        />
      </svg>
      <button
        v-for="placed in layout.nodes"
        :key="placed.node.id"
        type="button"
        class="group absolute flex items-center rounded-sm pl-[calc(var(--bar)+0.5rem)] text-left text-[0.8125rem] leading-tight focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring motion-safe:transition-opacity motion-safe:duration-200"
        :class="dimmed && !lit.has(placed.node.id) ? 'opacity-45' : ''"
        :style="{
          '--bar': `${NODE_WIDTH}px`,
          left: `${placed.x}px`,
          top: `${placed.y + placed.height / 2 - Math.max(placed.height, 10) / 2}px`,
          height: `${Math.max(placed.height, 10)}px`,
          width: `${NODE_WIDTH + labelWidth(placed.node.column)}px`,
        }"
        :aria-label="`${placed.node.label}, ${placed.node.figure}`"
        :aria-pressed="placed.node.id === selected"
        :title="`${placed.node.label} · ${placed.node.figure}`"
        @click="toggle(placed.node.id)"
      >
        <span
          class="flex min-w-0 items-baseline gap-1.5 rounded-sm bg-surface/85 px-1 py-0.5"
          :class="placed.height + GAP >= LABEL_HEIGHT || placed.node.id === selected ? '' : 'sr-only group-hover:not-sr-only group-focus-visible:not-sr-only'"
        >
          <span
            class="truncate"
            :class="placed.node.id === selected || (placed.node.tone === 'accent' && !dimmed) ? 'font-semibold text-accent-text' : placed.node.tone === 'quiet' ? 'text-muted' : 'font-medium text-primary'"
          >
            {{ placed.node.label }}
          </span>
          <span class="shrink-0 text-muted tabular-nums">{{ placed.node.figure }}</span>
        </span>
      </button>
    </div>
  </div>
</template>
