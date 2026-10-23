<script setup lang="ts">
/**
 * A heading row that starts a group in a time-ordered list, such as one day of sessions. The
 * records under it then show only what differs: the time, not the date again.
 */
defineProps<{
  colspan: number
}>()
</script>

<template>
  <tr
    role="row"
    class="border-b border-b-divider bg-fill/60"
  >
    <th
      scope="colgroup"
      :colspan="colspan"
      class="h-8 px-4 text-left text-xs font-medium text-muted"
    >
      <slot />
    </th>
  </tr>
</template>
