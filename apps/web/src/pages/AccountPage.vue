<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/pages/account/Layout.vue. Links are a fixed list
// instead of Laravel abilities; there is no route preloading.
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import { useSession } from '~/composables/useSession'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * The account area: the title row names the account, the links sit on the left and the page on
 * the right. A phone gets the same links as a scrollable row above the page.
 */
const { t } = useI18n()
const { account } = useSession()

const links = [
  { name: 'account-profile', labelKey: 'account.nav.profile', icon: 'lucide:user' },
  { name: 'account-preferences', labelKey: 'account.nav.preferences', icon: 'lucide:sliders-horizontal' },
  { name: 'account-security', labelKey: 'account.nav.security', icon: 'lucide:shield' },
  { name: 'account-api-keys', labelKey: 'account.nav.apiKeys', icon: 'lucide:key-round' },
] as const satisfies { name: string; labelKey: MessageKey; icon: string }[]
</script>

<template>
  <PageHeader
    :title="t('account.nav.label')"
    :description="account?.name"
  />
  <div class="grid w-full gap-6 lg:grid-cols-[12rem_minmax(0,1fr)] lg:gap-12">
    <aside class="grid content-start self-start lg:sticky lg:top-[calc(var(--shell-header-height)+var(--shell-gutter))]">
      <nav
        :aria-label="t('account.nav.label')"
        class="scroll-fade-x -mx-shell-gutter-x flex snap-x gap-1 overflow-x-auto px-shell-gutter-x lg:mx-0 lg:grid lg:px-0"
      >
        <RouterLink
          v-for="link in links"
          :key="link.name"
          :to="{ name: link.name }"
          class="flex h-shell-row shrink-0 snap-start items-center gap-2.5 rounded-lg px-3 text-sm font-medium text-muted transition-colors duration-200 ease-butter hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          active-class="bg-nav-active! text-nav-active-foreground!"
        >
          <Icon
            :name="link.icon"
            size="16"
            class="shrink-0"
            aria-hidden="true"
          />
          {{ t(link.labelKey) }}
        </RouterLink>
      </nav>
    </aside>
    <div class="max-w-4xl min-w-0">
      <RouterView />
    </div>
  </div>
</template>
