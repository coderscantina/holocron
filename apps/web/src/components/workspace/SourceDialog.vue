<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { ApiError } from '~/api'
import { hasUrl, linksResource, sourcePath, type SourceKind } from '~/api/resources/links'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { refreshLinks } from '~/composables/useLinks'
import type { Link, Pixel, WorkspaceId } from '~/generated'
import { DEFINITION_NAME_MAX_CHARS, SLUG_MAX_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'

/**
 * Creates or edits a link or pixel. Without `source` it creates one; an empty slug lets the
 * server pick one. Changing the slug of an existing one stops its old address at once.
 */
const props = defineProps<{ kind: SourceKind; workspace: WorkspaceId; source?: Link | Pixel | null }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const name = ref('')
const url = ref('')
const slug = ref('')
let intent = crypto.randomUUID()
watch([name, url, slug], () => {
  intent = crypto.randomUUID()
})

const editing = computed(() => props.source ?? null)
const isLink = computed(() => props.kind === 'link')
const renamed = computed(() => editing.value !== null && slug.value.trim() !== editing.value.slug)

watch(open, (value) => {
  if (value) {
    const source = editing.value
    name.value = source?.name ?? ''
    url.value = source && hasUrl(source) ? source.url : ''
    slug.value = source?.slug ?? ''
  } else {
    save.reset()
  }
})

const save = useMutation({
  mutationFn: async (): Promise<void> => {
    const source = editing.value
    const given = slug.value.trim() || undefined
    if (props.kind === 'link') {
      await (source
        ? linksResource.updateLink({ link: source.id, name: name.value, url: url.value, slug: slug.value }, intent)
        : linksResource.createLink({ workspace: props.workspace, name: name.value, url: url.value, slug: given }, intent))
      return
    }
    await (source
      ? linksResource.updatePixel({ pixel: source.id, name: name.value, slug: slug.value }, intent)
      : linksResource.createPixel({ workspace: props.workspace, name: name.value, slug: given }, intent))
  },
  onSuccess: async () => {
    open.value = false
    await refreshLinks(props.workspace)
  },
})

const error = computed(() => (save.error.value instanceof Error ? save.error.value : null))
/** A valid slug that is refused is held by another link or pixel. */
const slugError = computed(() => {
  const value = error.value
  if (value instanceof ApiError && value.body.errors?.slug?.[0]?.code === 'invalid_value') return t('links.slugTaken')
  return fieldMessage(value, 'slug')
})

const title = computed(() => {
  if (isLink.value) return editing.value ? t('links.editLink') : t('links.addLink')
  return editing.value ? t('links.editPixel') : t('links.addPixel')
})
const slugHint = computed(() =>
  renamed.value
    ? t('links.slugRenameHint', { path: sourcePath(props.kind, editing.value?.slug ?? '') })
    : t(editing.value ? 'links.slugHint' : 'links.slugCreateHint', { max: SLUG_MAX_CHARS })
)
const disabled = computed(() => !name.value.trim() || (isLink.value && !url.value.trim()) || (editing.value !== null && !slug.value.trim()))
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-md">
      <DialogHeaderCombined
        :title="title"
        :description="isLink ? t('links.linkDescription') : t('links.pixelDescription')"
      />
      <form
        class="grid gap-4"
        novalidate
        @submit.prevent="save.mutate()"
      >
        <FormField
          :id="`${kind}-name`"
          v-model="name"
          :label="t('settings.name')"
          :description="t('settings.nameHint', { max: DEFINITION_NAME_MAX_CHARS })"
          :error="fieldMessage(error, 'name')"
          required
        />
        <FormField
          v-if="isLink"
          id="link-url"
          v-model="url"
          type="url"
          autocomplete="off"
          :label="t('links.url')"
          :description="t('links.urlHint')"
          :error="fieldMessage(error, 'url')"
          required
        />
        <FormField
          :id="`${kind}-slug`"
          v-model="slug"
          autocomplete="off"
          :label="t('links.slug')"
          :description="slugHint"
          :error="slugError"
          :required="editing !== null"
        />
        <CommandAlert
          :shown="['name', 'url', 'slug']"
          :error="error"
        />
        <div class="flex justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            @click="open = false"
          >
            {{ t('actions.cancel') }}
          </Button>
          <Button
            type="submit"
            variant="primary"
            :loading="save.isPending.value"
            :disabled="disabled"
          >
            {{ editing ? t('actions.save') : title }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
