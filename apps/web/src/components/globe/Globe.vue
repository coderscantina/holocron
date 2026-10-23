<script setup lang="ts">
import { useElementSize, useMediaQuery } from '@vueuse/core'
import createGlobe, { type COBEOptions, type Globe, type Marker } from 'cobe'

import CountryFlag from '~/components/CountryFlag.vue'
import { useColorMode } from '~/composables/useColorMode'
import { densest, facing, nearestAngle, type Coordinates, type Orientation } from '~/lib/globe'

/** A flag pin on the globe. `id` is also its CSS anchor name, so it must be a CSS ident. */
export type GlobeMarker = Coordinates & {
  id: string
  country: string
  /** The pin's accessible name. */
  label: string
  /** The two lines shown while the pin is hovered or focused. */
  title: string
  detail: string
  /** What the pin holds; the globe rests facing where most of it is. */
  weight: number
  /** A small count on the pin. */
  badge?: string
  /** 0 to 1: draws an accent disc under the pin, sized by its share of the largest. */
  scale?: number
}

/** A place to turn to and mark with a halo; `seq` tells two spotlights at one place apart. */
export type GlobeSpotlight = Coordinates & { seq: number; id: string }

/**
 * A dotted globe (cobe) with a flag pin per marker. Pins are DOM buttons bound to cobe's
 * markers through CSS anchor positioning (`--cobe-<id>`), so they take the theme, the accent
 * and focus like any control. The globe rests facing where most weight is, turns to a
 * spotlight with a halo and the `spotlight` slot as its label, and turns by dragging. Clicking
 * a pin emits `select`. Under reduced motion it moves only when the person drags or focuses a pin.
 */
const props = defineProps<{ markers: GlobeMarker[]; spotlight: GlobeSpotlight | null; label: string }>()
const emit = defineEmits<{ select: [marker: GlobeMarker] }>()
defineSlots<{ spotlight?: () => unknown }>()

const { isDark } = useColorMode()
const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)')

/** How long the newest event keeps its label and halo before the globe turns back. */
const SPOTLIGHT_MS = 5000
/** Time constant of the turn toward a place: about 95% of the way after three of these. */
const EASE_MS = 280
const MAX_DRAG_THETA = 1.2
/**
 * cobe 2 draws only on `update()` and decodes its map texture after the first draw, so a new
 * globe keeps drawing for this long to show the land once it has loaded.
 */
const TEXTURE_MS = 600

const stage = useTemplateRef('stage')
const host = useTemplateRef('host')
const { width, height } = useElementSize(stage)
const size = computed(() => Math.floor(Math.min(width.value, height.value)))

const pins = computed(() => props.markers)
const spotlightId = computed(() => props.spotlight?.id ?? null)
const shownSpotlight = ref<GlobeSpotlight | null>(null)
const focused = ref<string | null>(null)
const hovered = ref<string | null>(null)
const inspected = computed(() => pins.value.find((pin) => pin.id === (hovered.value ?? focused.value)) ?? null)

const markers = computed<Marker[]>(() => {
  const list = pins.value.map((pin): Marker => ({ location: [pin.lat, pin.lng], size: 0, id: pin.id }))
  const spot = props.spotlight
  if (spot && !pins.value.some((pin) => pin.id === spot.id)) {
    list.push({ location: [spot.lat, spot.lng], size: 0, id: spot.id })
  }
  return list
})

type Theme = Pick<COBEOptions, 'dark' | 'diffuse' | 'mapBrightness' | 'mapBaseBrightness' | 'baseColor' | 'glowColor'>

/** Gray dots on a light or a dark sphere; the accent lives in the DOM pins, not the canvas. */
const theme = (dark: boolean): Theme =>
  dark
    ? { dark: 1, diffuse: 1.6, mapBrightness: 3, mapBaseBrightness: 0, baseColor: [0.32, 0.33, 0.36], glowColor: [0.1, 0.1, 0.12] }
    : { dark: 0, diffuse: 1.15, mapBrightness: 0.75, mapBaseBrightness: 0, baseColor: [1, 1, 1], glowColor: [0.93, 0.93, 0.95] }

let globe: Globe | null = null
let current: Orientation = facing(densest(props.markers))
let target: Orientation | null = null
let frame = 0
let last = 0
let drag: { pointer: number; x: number; y: number; from: Orientation } | null = null
let spotlightTimer: ReturnType<typeof setTimeout> | undefined
let drawUntil = 0

const rest = () => facing(densest(props.markers))

const render = (time: number) => {
  frame = 0
  const elapsed = last ? Math.min(time - last, 100) : 16
  last = time
  if (target && !drag) {
    const step = 1 - Math.exp(-elapsed / EASE_MS)
    current = {
      phi: current.phi + (target.phi - current.phi) * step,
      theta: current.theta + (target.theta - current.theta) * step,
    }
    if (Math.abs(target.phi - current.phi) < 1e-3 && Math.abs(target.theta - current.theta) < 1e-3) {
      current = target
      target = null
    }
  }
  globe?.update({ phi: current.phi, theta: current.theta })
  if ((target && !drag) || time < drawUntil) frame = requestAnimationFrame(render)
  else last = 0
}

const schedule = () => {
  if (!frame) frame = requestAnimationFrame(render)
}

/** Turns to `orientation`, gliding unless motion is reduced; then it jumps. */
const turnTo = (orientation: Orientation, glide: boolean) => {
  const next = { phi: nearestAngle(current.phi, orientation.phi), theta: orientation.theta }
  if (glide && !reducedMotion.value) target = next
  else {
    current = next
    target = null
  }
  schedule()
}

/** Automatic turns wait while the person drags or looks at a pin. */
const turnOnItsOwn = (orientation: Orientation) => {
  if (reducedMotion.value || drag || focused.value) return
  turnTo(orientation, true)
}

watch(size, (next) => {
  if (next <= 0) return
  if (globe) {
    globe.update({ width: next, height: next })
    return
  }
  const canvas = document.createElement('canvas')
  canvas.className = 'block size-full'
  host.value?.append(canvas)
  globe = createGlobe(canvas, {
    devicePixelRatio: Math.min(2, window.devicePixelRatio || 1),
    width: next,
    height: next,
    phi: current.phi,
    theta: current.theta,
    mapSamples: 16_000,
    markerColor: [0, 0, 0],
    markers: markers.value,
    markerElevation: 0,
    ...theme(isDark.value),
  })
  drawUntil = performance.now() + TEXTURE_MS
  schedule()
})

watch(markers, (next) => globe?.update({ markers: next }))
watch(isDark, (dark) => globe?.update(theme(dark)))

watch(
  () => props.spotlight,
  (spot) => {
    if (!spot) return
    shownSpotlight.value = spot
    turnOnItsOwn(facing(spot))
    clearTimeout(spotlightTimer)
    spotlightTimer = setTimeout(() => {
      shownSpotlight.value = null
      turnOnItsOwn(rest())
    }, SPOTLIGHT_MS)
  }
)

const startDrag = (event: PointerEvent) => {
  if (event.button !== 0 || (event.target instanceof Element && event.target.closest('button'))) return
  stage.value?.setPointerCapture(event.pointerId)
  drag = { pointer: event.pointerId, x: event.clientX, y: event.clientY, from: current }
  target = null
}

const moveDrag = (event: PointerEvent) => {
  if (!drag || event.pointerId !== drag.pointer || size.value <= 0) return
  const scale = Math.PI / size.value
  const theta = drag.from.theta + (event.clientY - drag.y) * scale
  current = {
    phi: drag.from.phi + (event.clientX - drag.x) * scale,
    theta: Math.max(-MAX_DRAG_THETA, Math.min(MAX_DRAG_THETA, theta)),
  }
  schedule()
}

const endDrag = (event: PointerEvent) => {
  if (drag?.pointer === event.pointerId) drag = null
}

const inspect = (pin: GlobeMarker) => {
  focused.value = pin.id
  turnTo(facing(pin), true)
}

onBeforeUnmount(() => {
  cancelAnimationFrame(frame)
  clearTimeout(spotlightTimer)
  globe?.destroy()
  globe = null
})

const anchor = (id: string) => `position-anchor: --cobe-${id}`
/** cobe sets `--cobe-visible-<id>` only while a marker faces the viewer; behind the globe it falls back. */
const facingOnly = (id: string) => `opacity: var(--cobe-visible-${id}, 0); pointer-events: var(--cobe-visible-${id}, none)`
</script>

<template>
  <div
    ref="stage"
    class="relative size-full cursor-grab touch-pan-y select-none active:cursor-grabbing lg:touch-none"
    @pointerdown="startDrag"
    @pointermove="moveDrag"
    @pointerup="endDrag"
    @pointercancel="endDrag"
  >
    <div class="absolute inset-0 grid place-items-center">
      <div
        class="relative"
        :style="{ width: `${size}px`, height: `${size}px` }"
      >
        <div
          ref="host"
          class="size-full"
        />
        <div
          v-if="shownSpotlight && spotlightId"
          :key="shownSpotlight.seq"
          class="pointer-events-none absolute top-[anchor(center)] left-[anchor(center)] size-[30%] -translate-1/2 transition-opacity duration-500"
          :style="`${anchor(spotlightId)}; ${facingOnly(spotlightId)}`"
          aria-hidden="true"
        >
          <span class="globe-halo block size-full rounded-full border border-accent/20" />
        </div>
        <div
          role="group"
          :aria-label="label"
        >
          <div
            v-for="pin in pins"
            :key="pin.id"
            class="absolute top-[anchor(center)] left-[anchor(center)] size-0 transition-opacity duration-300"
            :class="(hovered ?? focused) === pin.id ? 'z-20' : 'z-10'"
            :style="`${anchor(pin.id)}; ${facingOnly(pin.id)}`"
          >
            <span
              v-if="pin.scale !== undefined"
              class="absolute -translate-1/2 rounded-full bg-accent/15 ring-1 ring-accent/45"
              :style="{ width: `${0.75 + pin.scale * 4.5}rem`, height: `${0.75 + pin.scale * 4.5}rem` }"
              aria-hidden="true"
            />
            <span
              class="absolute size-1.5 -translate-1/2 rounded-full bg-accent ring-2 ring-card"
              aria-hidden="true"
            />
            <button
              type="button"
              class="absolute bottom-2.5 left-0 grid size-7.5 -translate-x-1/2 cursor-pointer place-items-center rounded-full bg-card shadow-soft ring-1 ring-hairline transition-transform hover:scale-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
              :aria-label="pin.label"
              @pointerenter="hovered = pin.id"
              @pointerleave="hovered = null"
              @focus="inspect(pin)"
              @blur="focused = null"
              @click="inspect(pin), emit('select', pin)"
            >
              <span class="grid size-6 place-items-center overflow-hidden rounded-full">
                <CountryFlag
                  :code="pin.country"
                  class="scale-[2.1]"
                />
              </span>
              <span
                v-if="pin.badge"
                class="absolute -top-1.5 -right-1.5 grid h-4 min-w-4 place-items-center rounded-full bg-primary px-1 text-2xs font-semibold whitespace-nowrap text-primary-foreground tabular-nums"
                aria-hidden="true"
              >
                {{ pin.badge }}
              </span>
            </button>
          </div>
        </div>
        <div
          v-if="inspected"
          class="pointer-events-none absolute bottom-[calc(anchor(top)+3rem)] left-[anchor(center)] z-30 -translate-x-1/2 rounded-md border border-popover-border bg-popover px-3 py-1.5 text-xs whitespace-nowrap text-popover-foreground shadow-soft-lg"
          :style="`${anchor(inspected.id)}; ${facingOnly(inspected.id)}`"
          aria-hidden="true"
        >
          <p class="font-semibold text-primary">{{ inspected.title }}</p>
          <p class="text-muted tabular-nums">{{ inspected.detail }}</p>
        </div>
        <div
          v-else-if="shownSpotlight && spotlightId && $slots.spotlight"
          :key="shownSpotlight.seq"
          class="pointer-events-none absolute bottom-[calc(anchor(top)+3rem)] left-[anchor(center)] z-30 flex max-w-64 -translate-x-1/2 items-center gap-2 rounded-md border border-popover-border bg-popover px-2.5 py-1 text-sm whitespace-nowrap text-popover-foreground shadow-soft-lg animate-in fade-in-0"
          :style="`${anchor(spotlightId)}; ${facingOnly(spotlightId)}`"
          aria-hidden="true"
        >
          <slot name="spotlight" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.globe-halo {
  background: radial-gradient(circle, color-mix(in oklch, var(--accent) 22%, transparent), transparent 70%);
}
</style>
