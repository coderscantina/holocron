<script setup lang="ts">
import { RadioGroupItem, RadioGroupRoot } from 'reka-ui'

import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { useI18n } from '~/plugins/i18n'

import { setupPaths, type SetupPath } from './steps'

const emit = defineEmits<{ submit: [path: SetupPath] }>()
const path = defineModel<SetupPath>({ required: true })

const { t } = useI18n()

/** Restore comes with its own steps in a later build. */
const available = new Set<SetupPath>(['fresh', 'import'])

const icons: Record<SetupPath, string> = {
  fresh: 'lucide:plus',
  import: 'lucide:download',
  restore: 'lucide:history',
}
</script>

<template>
  <form
    class="grid gap-7"
    @submit.prevent="emit('submit', path)"
  >
    <h1
      id="setup-path-title"
      class="text-4xl font-semibold tracking-tight text-primary"
    >
      {{ t('setup.path.title') }}
      <span class="block font-medium text-muted">{{ t('setup.path.subtitle') }}</span>
    </h1>
    <div class="grid gap-3.5">
      <RadioGroupRoot
        v-model="path"
        class="grid gap-2"
        aria-labelledby="setup-path-title"
      >
        <RadioGroupItem
          v-for="option in setupPaths"
          :key="option"
          :value="option"
          :disabled="!available.has(option)"
          class="group flex cursor-pointer disabled:cursor-not-allowed disabled:opacity-60 items-start gap-3.5 rounded-[var(--radius-control)] bg-fill p-4 text-left transition-colors duration-200 ease-butter focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring data-[state=checked]:bg-surface data-[state=checked]:inset-ring data-[state=checked]:inset-ring-accent"
        >
          <span
            class="grid size-9 shrink-0 place-items-center rounded-[var(--radius-control)] bg-fill-strong text-foreground group-data-[state=checked]:bg-accent/15 group-data-[state=checked]:text-accent-text"
          >
            <Icon
              :name="icons[option]"
              aria-hidden="true"
            />
          </span>
          <span class="grid gap-0.5">
            <span class="text-[15px] font-semibold text-primary">{{ t(`setup.path.${option}.title`) }}</span>
            <span class="text-sm text-muted">{{ t(`setup.path.${option}.description`) }}</span>
            <span
              v-if="!available.has(option)"
              class="text-sm text-muted"
            >
              {{ t('setup.path.later') }}
            </span>
          </span>
        </RadioGroupItem>
      </RadioGroupRoot>
      <Button
        type="submit"
        variant="accent"
        size="lg"
      >
        {{ t('setup.continue') }}
      </Button>
    </div>
  </form>
</template>
