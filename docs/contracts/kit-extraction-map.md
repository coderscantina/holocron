# Kit extraction map

What `apps/web` took from Kit and Folio, what changed, and which Laravel dependency each change replaced. Sources are pinned in `sources/manifest.json`: Kit `f447e797` and Folio `219e9aaa`, both MIT. Adapted files name their source in a header comment. The web app builds without either checkout.

Dependency versions are copied from Kit's `bun.lock`: vue 3.5.42, vue-router 5.3.1, @tanstack/vue-query 5.102.8, @vueuse/core 14.4.0, reka-ui 2.10.4, vue-i18n 11.4.10, tailwindcss 4.3.3, vite 8.2.2, vue-tsc 3.3.11, vitest 4.1.11, oxlint 1.81.0.

## Copied unchanged

| Kit source | Holocron path |
| --- | --- |
| `components/ui/{button,card,dropdown-menu,sheet,tooltip,dialog,command,badge,skeleton,input,label,separator,spinner,empty-state,alert}` | `apps/web/src/components/ui/` |
| `components/ui/overlay.ts` | same |
| `components/Icon.vue`, `lib/utils.ts`, `lib/shortcuts.ts`, `composables/useShortcuts.ts`, `composables/useCommandPalette.ts` | same names |
| `components/app/SkipLink.vue`, `components/app/AppSidebarResizer.vue`, `components/KeyboardShortcutsDialog.vue` | same names; the dialog drops its feature flag |
| `components/ui/{alert-dialog,select,checkbox,radio-group}`, `components/ConfirmDialog.vue`, `composables/useConfirm.ts`, `components/account/{SettingsPage,SettingsSection}.vue`, `assets/css/vue-sonner.css` (milestone 10) | same names; `vue-sonner` 2.0.9 and `qrcode` 1.5.4 added at Kit's versions |

Removed from the copied primitives because nothing uses them: `SplitButton`, `SplitBadge`, `AvatarList`, `UserAvatar`, `UnobstrusiveSheetContent`, `TooltipIcon`, presence dot variants.

## Adapted

| Kit or Folio source | Holocron path | Change | Replaced dependency |
| --- | --- | --- | --- |
| `assets/css/app.css` | `src/assets/css/app.css` | Token ladder, type scale, shell geometry, density, focus and reduced motion unchanged. Accent presets, AI animation, toast sheet, stacked tables and unused utilities removed. Chart series tokens added (`series-primary`, `series-secondary`, `series-comparison`, `series-grid`). | Blade inline accent script |
| `layouts/AppLayout.vue` | `src/layouts/AppLayout.vue` | Route focus fires only when the path changes, so later exploration edits keep control focus. Switching sites cancels the previous site's queries. Impersonation banner and page-search shortcut removed. | Laravel impersonation |
| `components/app/AppHeader.vue` | same | Breadcrumbs replaced by the workspace / site context switcher. Notification bell removed. Palette trigger and user menu keep their places. | Echo notifications |
| (new, Clerk-style reference) | `src/components/app/AppContextSwitcher.vue` | Workspace and site selectors with slash separator, kind badge, paired chevrons, radio check marks, empty states, truncation with full accessible names. Options come from `workspaces.list`. | Client-side membership lists |
| `components/app/AppSidebar.vue`, `AppSidebarNav.vue`, `AppNavLink.vue`, `AppBrand.vue` | same | Sections come from the site context, not from Laravel abilities. Site settings sit at the bottom. Brand and version come from the runtime config. | `App.Data.MeData`, generated abilities |
| `components/app/AppMobileSidebar.vue` | same | The account block is removed: identity and account links stay in the header menu on every width. | `useAuth` |
| `components/app/AppTabBar.vue` | same | Menu tab first, then overview, explore, realtime. | `useAuth`, access-control abilities |
| `components/app/AppUserMenu.vue` | same | Identity from `auth.session`. One account link, appearance and language submenus, shortcuts, sign out. PWA install removed. | `useAuth`, Laravel logout route |
| `components/CommandPalette.vue` | same | Pages and site switching from the same authorized lists as the sidebar. | access-control abilities |
| `components/ui/command/Command.vue` | same | Keeps the first match highlighted while a search is typed. Without it, a list that shrinks under a resting pointer drops its highlight and Enter does nothing (found in the browser journey). | none |
| `components/ErrorBoundary.vue`, `components/FormField.vue` | same | ErrorBoundary uses EmptyState. FormField drops field presence and the password reveal. | Laravel presence channel |
| `composables/useSidebar.ts`, `useColorMode.ts`, `usePageMeta.ts` | same | Storage keys under `holocron:`. Page meta keeps title and announcement, no breadcrumbs. | Blade color-mode script (now inline in `index.html`) |
| `plugins/i18n.ts` | same | `t` accepts only `MessageKey`; generated enums map to keys through exhaustive records. Locale choice persists. | Blade `<html lang>` |
| `plugins/vue-query.ts` | same | No focus refetch. Retries only transport failures. | Laravel 419 handling |
| `api/client.ts` | `src/api/client.ts` | Rewritten as the generated command/query transport (see below). | CSRF cookie priming, 419 retry, Echo socket header, Laravel query encoding |
| Folio `services/runtime-config.ts` | `src/services/runtime-config.ts` | Typed by the generated `RuntimeConfig`. No offline cache. | Folio Clerk key |
| `components/ui/table/*`, `components/ui/TableEmptyRow.vue`, `TableLoadingRow.vue` | same | No stacked phone layout (the stylesheet has none): wide tables scroll in their box. `TableSortableHead` left out (no `useTableQueryState`). The empty row needs a label. | none |
| `components/PasswordConfirmDialog.vue`, `composables/useStepUp.ts` | `src/components/ReauthDialog.vue`, `src/composables/useReauth.ts` | Asks before the command, from the generated `reauth` metadata, instead of retrying a 423; adds the second factor; one dialog for the app. | Laravel password confirmation |
| `components/account/{TwoFactorSetupDialog,TwoFactorSection,BackupCodesDisplay,PasswordSection,AccessTokenDialog,AccessTokensSection}.vue`, `pages/account/Layout.vue`, `components/people/{PeopleTable,InviteDialog}.vue` | `src/components/account/*`, `src/pages/AccountPage.vue`, `src/pages/workspace/MembersSection.vue`, `src/components/workspace/InviteDialog.vue` | Generated access operations; withheld one-time secrets have their own state; invitations are links, not email. | Laravel auth, Sanctum tokens, mail invites |
| Folio `apps/server/src/web.rs` | `apps/server/src/http/web.rs` | Same cache split; a missing hashed asset is a 404 instead of `index.html`. | none |

## Replaced by Holocron code

| Kit concern | Holocron replacement |
| --- | --- |
| `api/resources/*` REST resources, `App.Data.*` types | `src/api/resources/{session,sites}.ts` over the generated `Commands` and `Queries` maps |
| `useAuth`, abilities, route access requirements | `auth.session` query, server-provided `capabilities` on each site, router guard |
| Laravel validation error shape | `ApiErrorBody.errors` with `FieldErrorCode`, translated through exhaustive maps |
| Echo, reactive layer, notifications | Not needed yet; live snapshots follow the architecture pattern in later milestones |
| `useTableQueryState`, data tables, saved views | Not extracted yet. Milestone 06 extracts them with the exploration state. |

## Transport

`src/api/client.ts` is the only module that calls `fetch` (enforced by an oxlint `no-restricted-globals` rule; `services/runtime-config.ts` is the one other exception, for `/config.json`). It sends the request marker on every call, adds a UUID idempotency key when the caller gives none, passes `AbortSignal`, throws `ApiError` with the typed body for every non-2xx answer, cuts non-JSON bodies to 500 characters, falls back to `HTTP <status>`, throws `NetworkError` when no answer arrives, and calls the auth handler only for `unauthenticated`.

## Environment selector

The product contract defines no environment setting, so the header has no environment control and no static environment badge (kit-implementation.md, Header and navigation). The workspace kind badge (Team, Personal) is real data and takes the badge position. If an environment concept is ever added, it needs a decision record and a protocol type first.

## Known gaps against Kit

- The full lucide icon set is bundled (97 KB gzipped, one cached chunk), as in Kit. A curated subset would cut most of it.
- Exploration controls, tables with sort and paging, saved views and charts beyond the overview fixture are milestone 06 work.
