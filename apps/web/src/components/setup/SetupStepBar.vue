<script setup lang="ts">
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{
  /** 1-based. */
  current: number
  total: number
}>()

const { t } = useI18n()

const bars = computed(() =>
  Array.from({ length: props.total }, (_, index) => {
    if (index + 1 < props.current) return 'bg-muted'
    return index + 1 === props.current ? 'bg-accent' : 'bg-fill-strong'
  })
)
</script>

<template>
  <div class="flex items-center gap-1.5 text-sm text-muted">
    <span
      v-for="(bar, index) in bars"
      :key="index"
      class="h-[3px] w-[18px] rounded-full"
      :class="bar"
      aria-hidden="true"
    />
    <span class="ml-1.5">{{ t('setup.stepOf', { current, total }) }}</span>
  </div>
</template>
