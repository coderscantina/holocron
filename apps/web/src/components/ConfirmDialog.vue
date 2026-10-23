<script setup lang="ts">
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '~/components/ui/alert-dialog'
import { useConfirm } from '~/composables/useConfirm'

const { open, current, answer, dismiss, labels } = useConfirm()
</script>

<template>
  <AlertDialog
    v-if="current"
    :open="open"
    @update:open="(value) => !value && dismiss()"
  >
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle v-if="current.options.title">
          {{ current.options.title }}
        </AlertDialogTitle>
        <AlertDialogDescription>{{ current.options.message }}</AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel @click="answer(false)">
          {{ labels.cancel(current.options) }}
        </AlertDialogCancel>
        <AlertDialogAction
          :variant="current.options.variant ?? 'primary'"
          @click="answer(true)"
        >
          {{ labels.confirm(current.options) }}
        </AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>
