import type { MessageKey } from '~/plugins/i18n'

/** Workspace-wide areas. */
export const workspaceAreas = {
  boards: { labelKey: 'nav.boards', icon: 'lucide:layout-grid' },
} as const satisfies Record<string, { labelKey: MessageKey; icon: string }>
