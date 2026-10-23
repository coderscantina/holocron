import { createSharedComposable } from '@vueuse/core'
import { nextTick, ref, shallowRef } from 'vue'

import type { ButtonVariants } from '~/components/ui/button/variants'
import { useI18n } from '~/plugins/i18n'

export interface ConfirmOptions {
  /** Heading. Omitted renders the message alone. */
  title?: string
  message: string
  confirmLabel?: string
  cancelLabel?: string
  /** Button variant for the confirming action; `destructive` for deletions. */
  variant?: ButtonVariants['variant']
}

interface PendingConfirm {
  options: ConfirmOptions
  settle: (value: boolean) => void
}

/** Matches the alert dialog's close animation, so the next dialog does not cut it short. */
const CLOSE_ANIMATION_MS = 300

const useConfirmBase = () => {
  const { t } = useI18n()

  const open = ref(false)
  const current = shallowRef<PendingConfirm | null>(null)

  // Overwriting a live dialog used to tear down its markup while its resolver
  // stayed captured in an unreachable closure, so the first promise never
  // settled. A second request queues behind the first instead.
  const queue: PendingConfirm[] = []

  let closeTimeout: ReturnType<typeof setTimeout> | null = null

  const show = (entry: PendingConfirm) => {
    if (closeTimeout) {
      clearTimeout(closeTimeout)
      closeTimeout = null
    }

    current.value = entry
    // A tick later, so the dialog mounts closed and its enter animation plays;
    // setting both in one tick renders it already open.
    void nextTick(() => {
      open.value = true
    })
  }

  const close = () => {
    open.value = false

    if (closeTimeout) clearTimeout(closeTimeout)

    closeTimeout = setTimeout(() => {
      current.value = null
      closeTimeout = null

      const next = queue.shift()
      if (next) show(next)
    }, CLOSE_ANIMATION_MS)
  }

  /** Answers the open dialog from one of its buttons. */
  const answer = (value: boolean) => {
    current.value?.settle(value)
    close()
  }

  /**
   * Escape, an overlay click — and also the close the primitive performs for
   * its own action buttons. Without this the promise stayed unsettled and
   * every `await confirm(...)` the user escaped out hung forever.
   *
   * The `false` waits a microtask because the primitive closes itself before
   * the button's own click handler runs: settling here and now would beat a
   * real answer to it and report every confirmation as a cancel. `settle` is
   * idempotent, so whichever answer lands first is the one that counts.
   */
  const dismiss = () => {
    const entry = current.value

    void Promise.resolve().then(() => entry?.settle(false))
    close()
  }

  const confirm = (options: ConfirmOptions): Promise<boolean> =>
    new Promise<boolean>((resolve) => {
      let settled = false

      const entry: PendingConfirm = {
        options,
        settle: (value) => {
          if (settled) return
          settled = true
          resolve(value)
        },
      }

      // `current` outlives `open` by the close animation, and precedes it by a
      // tick on the way in, so it is the honest "a dialog owns the slot" flag.
      if (current.value) {
        queue.push(entry)
        return
      }

      show(entry)
    })

  const labels = {
    confirm: (options: ConfirmOptions) => options.confirmLabel ?? t('actions.confirm'),
    cancel: (options: ConfirmOptions) => options.cancelLabel ?? t('actions.cancel'),
  }

  return { open, current, confirm, answer, dismiss, labels }
}

/**
 * One dialog for the whole app, rendered by `ConfirmDialog.vue` in `app.vue`.
 * Shared so any call site awaits the same instance instead of mounting a
 * dialog of its own.
 */
export const useConfirm = createSharedComposable(useConfirmBase)
