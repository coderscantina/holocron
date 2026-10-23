<script setup lang="ts">
import { useRouter } from 'vue-router'

import CommandResultGroup from '~/components/CommandResultGroup.vue'
import { useCommandPalette } from '~/composables/useCommandPalette'
import { useBoards } from '~/composables/useDefinitions'
import { useSiteContext } from '~/composables/useSiteContext'
import { useI18n } from '~/plugins/i18n'

/** Boards of the workspace in the header context, read through the cache the Boards page shares. */
const { t } = useI18n()
const router = useRouter()
const palette = useCommandPalette()
const context = useSiteContext()
const boards = useBoards(() => (context.workspace.value ?? context.workspaces.value[0])?.id ?? null)
const items = computed(() => (boards.list.data.value?.boards ?? []).map((board) => ({ id: board.id, label: board.name })))
</script>

<template>
  <CommandResultGroup
    :heading="t('commandPalette.boards')"
    icon="lucide:layout-grid"
    :items="items"
    @select="(board) => (palette.close(), router.push({ name: 'board', params: { boardId: board.id } }))"
  />
</template>
