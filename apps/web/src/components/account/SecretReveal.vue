<script setup lang="ts">
import { toast } from 'vue-sonner'

import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { useI18n } from '~/plugins/i18n'

/**
 * A secret the server discloses once: an API key, an invitation or share link, a generated
 * password. A replayed answer withholds it (`secret` is `null`), and this says so instead of
 * showing an empty field, since the secret cannot be shown again.
 */
const props = defineProps<{
  id: string
  label: string
  secret: string | null
}>()

const { t } = useI18n()

const copy = async () => {
  if (!props.secret) return
  await navigator.clipboard.writeText(props.secret)
  toast.success(t('secrets.copied'))
}
</script>

<template>
  <div class="grid gap-3">
    <template v-if="secret">
      <Alert color="warning">
        <p class="text-sm">{{ t('secrets.once') }}</p>
      </Alert>
      <div class="grid gap-1.5">
        <Label :for="id">{{ label }}</Label>
        <div class="flex gap-2">
          <Input
            :id="id"
            :model-value="secret"
            readonly
            class="font-mono"
            @focus="($event.target as HTMLInputElement).select()"
          />
          <Button
            type="button"
            variant="outline"
            @click="copy"
          >
            <Icon name="lucide:copy" />
            {{ t('secrets.copy') }}
          </Button>
        </div>
      </div>
    </template>
    <Alert
      v-else
      color="info"
    >
      <p class="text-sm">{{ t('secrets.withheld') }}</p>
    </Alert>
  </div>
</template>
