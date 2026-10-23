<script setup lang="ts">
import type { Share } from '~/composables/useSharedView'
import type { SharedView } from '~/generated'

import ShareBoard from './ShareBoard.vue'
import ShareExploration from './ShareExploration.vue'

/** The share's own view: an exploration or a board. */
defineProps<{ share: Share; view: SharedView }>()
</script>

<template>
  <ShareExploration
    v-if="view.target.kind === 'exploration'"
    :share="share"
    :view="view"
  />
  <ShareBoard
    v-else
    :share="share"
    :view="view"
    :board-id="view.target.board"
  />
</template>
