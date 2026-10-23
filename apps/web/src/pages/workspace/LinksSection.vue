<script setup lang="ts">
import { useLinks } from '~/composables/useLinks'
import type { TeamDetail } from '~/generated'

import SourceTable from './SourceTable.vue'

/** The workspace's links and pixels (decision 0020), each list with its own actions. */
const props = defineProps<{ detail: TeamDetail; revision: number }>()

const workspace = computed(() => props.detail.team.id)
const canCreate = computed(() => props.detail.team.capabilities.includes('workspace.write'))
const { links, pixels } = useLinks(workspace)
</script>

<template>
  <div class="grid gap-8">
    <SourceTable
      kind="link"
      :workspace="workspace"
      :items="links.data.value?.links ?? []"
      :capabilities="links.data.value?.capabilities ?? []"
      :can-create="canCreate"
      :loading="links.isPending.value"
      :error="links.error.value"
    />
    <SourceTable
      kind="pixel"
      :workspace="workspace"
      :items="pixels.data.value?.pixels ?? []"
      :capabilities="pixels.data.value?.capabilities ?? []"
      :can-create="canCreate"
      :loading="pixels.isPending.value"
      :error="pixels.error.value"
    />
  </div>
</template>
