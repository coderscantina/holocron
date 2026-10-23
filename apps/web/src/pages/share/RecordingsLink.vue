<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { useI18n } from '~/plugins/i18n'

/** Opens the share's replays. Shown only when the share grants recordings. */
const { t } = useI18n()
const route = useRoute()
</script>

<template>
  <Button
    as-child
    variant="outline"
    size="sm"
  >
    <RouterLink :to="{ name: 'share-replays', params: { token: route.params.token } }">
      <Icon
        name="lucide:circle-play"
        aria-hidden="true"
      />
      {{ t('share.replays') }}
    </RouterLink>
  </Button>
</template>
