import { createSharedComposable } from '@vueuse/core'
import { ref, shallowRef } from 'vue'

import { ApiError } from '~/api'
import type { Commands } from '~/generated/commands'
import { needsPrompt, type ReauthCommand } from '~/lib/reauth'

import { currentSession } from './useSession'

/**
 * The password prompt in front of sensitive commands (decision 0007). One dialog for the app,
 * rendered by `ReauthDialog.vue` in `App.vue`; any caller awaits the same instance.
 */
export const useReauth = createSharedComposable(() => {
  const open = ref(false)
  const pending = shallowRef<((confirmed: boolean) => void) | null>(null)

  /** Opens the dialog; resolves `true` once the server accepted the password, `false` on cancel. */
  const prompt = () =>
    new Promise<boolean>((resolve) => {
      pending.value?.(false)
      pending.value = resolve
      open.value = true
    })

  /** Called by the dialog. */
  const settle = (confirmed: boolean) => {
    const resolve = pending.value
    pending.value = null
    open.value = false
    resolve?.(confirmed)
  }

  /**
   * Runs a sensitive command: asks for the password first when the session's check is missing
   * or about to lapse, and once more if the server still says `reauthentication_required`.
   * Resolves `null` when the person cancels.
   */
  const run = async <N extends ReauthCommand, T>(
    name: N,
    args: Commands[N]['args'],
    command: () => Promise<T>
  ): Promise<T | null> => {
    if (needsPrompt(name, args, currentSession().reauthenticated_until) && !(await prompt())) return null
    try {
      return await command()
    } catch (error) {
      if (!(error instanceof ApiError && error.code === 'reauthentication_required')) throw error
      if (!(await prompt())) return null
      return command()
    }
  }

  return { open, prompt, settle, run }
})
