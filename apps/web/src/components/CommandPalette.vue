<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/CommandPalette.vue. Offers the same
// authorized navigation as the sidebar, plus every site the caller may switch to.
import CommandPaletteBoards from '~/components/CommandPaletteBoards.vue'
import CommandPaletteSite from '~/components/CommandPaletteSite.vue'
import Icon from '~/components/Icon.vue'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from '~/components/ui/command'
import { DialogDescription, DialogTitle } from '~/components/ui/dialog'
import { useColorMode } from '~/composables/useColorMode'
import { useCommandPalette } from '~/composables/useCommandPalette'
import { useNavigation } from '~/composables/useNavigation'
import { useSignOut } from '~/composables/useSession'
import { useShortcut, useShortcutHelp } from '~/composables/useShortcuts'
import { useSidebar } from '~/composables/useSidebar'
import { useSiteContext } from '~/composables/useSiteContext'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const router = useRouter()
const palette = useCommandPalette()
const sidebar = useSidebar()
const { isDark, setMode } = useColorMode()
const shortcutHelp = useShortcutHelp()
const signOut = useSignOut()
const { items, settings } = useNavigation()
const context = useSiteContext()

interface Action {
  id: string
  label: string
  icon: string
  shortcut?: string
  run: () => void
}

const actions = computed<Action[]>(() => [
  {
    id: 'theme',
    label: isDark.value ? t('commandPalette.lightMode') : t('commandPalette.darkMode'),
    icon: isDark.value ? 'lucide:sun' : 'lucide:moon',
    run: () => setMode(isDark.value ? 'light' : 'dark'),
  },
  {
    id: 'sidebar',
    label: t('shortcuts.toggleSidebar'),
    icon: 'lucide:panel-left',
    shortcut: '⌘B',
    run: () => sidebar.toggle(),
  },
  { id: 'shortcuts', label: t('shortcuts.title'), icon: 'lucide:keyboard', run: () => shortcutHelp.show() },
  { id: 'sign-out', label: t('auth.signOut'), icon: 'lucide:log-out', run: () => void signOut() },
])

const sites = computed(() =>
  context.workspaces.value.flatMap((workspace) =>
    workspace.sites.map((site) => ({ site, workspace: workspace.name }))
  )
)

useShortcut({
  keys: 'mod+k',
  description: () => t('commandPalette.open'),
  allowInInput: true,
  allowInOverlay: true,
  handler: () => palette.toggle(),
})

const run = (action: () => void) => {
  palette.close()
  action()
}
</script>

<template>
  <CommandDialog v-model:open="palette.isOpen.value">
    <DialogTitle class="sr-only">{{ t('commandPalette.title') }}</DialogTitle>
    <DialogDescription class="sr-only">{{ t('commandPalette.description') }}</DialogDescription>
    <CommandInput :placeholder="t('commandPalette.placeholder')" />
    <CommandList>
      <CommandEmpty>{{ t('commandPalette.empty') }}</CommandEmpty>
      <CommandPaletteSite
        v-if="context.siteId.value"
        :site-id="context.siteId.value"
      />
      <CommandPaletteBoards />
      <CommandGroup :heading="t('commandPalette.navigation')">
        <CommandItem
          v-for="item in [...items, ...(settings ? [settings] : [])]"
          :key="item.id"
          :value="`${t(item.labelKey)} ${item.id}`"
          @select="run(() => router.push(item.to))"
        >
          <Icon :name="item.icon" />
          {{ t(item.labelKey) }}
        </CommandItem>
      </CommandGroup>
      <CommandGroup
        v-if="sites.length > 0"
        :heading="t('commandPalette.sites')"
      >
        <CommandItem
          v-for="entry in sites"
          :key="entry.site.id"
          :value="`${entry.site.name} ${entry.site.domain} ${entry.workspace}`"
          @select="run(() => context.selectSite(entry.site))"
        >
          <Icon name="lucide:globe" />
          <span class="min-w-0 flex-1 truncate">{{ entry.site.name }}</span>
          <span class="shrink-0 truncate text-xs text-muted in-data-highlighted:text-accent-foreground">{{ entry.workspace }}</span>
        </CommandItem>
      </CommandGroup>
      <CommandGroup :heading="t('commandPalette.actions')">
        <CommandItem
          v-for="action in actions"
          :key="action.id"
          :value="action.label"
          @select="run(action.run)"
        >
          <Icon :name="action.icon" />
          {{ action.label }}
          <CommandShortcut v-if="action.shortcut">{{ action.shortcut }}</CommandShortcut>
        </CommandItem>
      </CommandGroup>
    </CommandList>
  </CommandDialog>
</template>
