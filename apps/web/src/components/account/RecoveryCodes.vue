<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/account/BackupCodesDisplay.vue. A
// replayed answer withholds the codes, which says so instead of an empty list.
import { toast } from 'vue-sonner'

import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useI18n } from '~/plugins/i18n'

/** Recovery codes are shown once. Copy and download are the two ways to keep them. */
const props = defineProps<{ codes: string[] | null }>()

const { t } = useI18n()

const asText = () => (props.codes ?? []).join('\n')

const copy = async () => {
  await navigator.clipboard.writeText(asText())
  toast.success(t('twoFactor.codesCopied'))
}

const download = () => {
  const url = URL.createObjectURL(new Blob([asText()], { type: 'text/plain' }))
  const link = document.createElement('a')
  link.href = url
  link.download = 'holocron-recovery-codes.txt'
  link.click()
  URL.revokeObjectURL(url)
}
</script>

<template>
  <div
    v-if="codes"
    class="grid gap-3"
  >
    <Alert color="warning">
      <p class="text-sm">{{ t('twoFactor.codesOnce') }}</p>
    </Alert>
    <ul
      class="grid grid-cols-2 gap-2"
      :aria-label="t('twoFactor.recoveryCodes')"
    >
      <li
        v-for="code in codes"
        :key="code"
        class="rounded-md border border-hairline px-2 py-1 text-center font-mono text-sm"
      >
        {{ code }}
      </li>
    </ul>
    <div class="grid grid-cols-2 gap-2">
      <Button
        type="button"
        @click="copy"
      >
        <Icon name="lucide:copy" />
        {{ t('secrets.copy') }}
      </Button>
      <Button
        type="button"
        @click="download"
      >
        <Icon name="lucide:download" />
        {{ t('twoFactor.download') }}
      </Button>
    </div>
  </div>
  <Alert
    v-else
    color="info"
  >
    <p class="text-sm">{{ t('twoFactor.codesWithheld') }}</p>
  </Alert>
</template>
