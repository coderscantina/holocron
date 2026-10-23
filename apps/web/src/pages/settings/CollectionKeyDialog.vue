<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { sitesResource } from '~/api/resources/sites'
import SecretReveal from '~/components/account/SecretReveal.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { CollectionKeyCreated, CollectionKeysCreateArgs, Site } from '~/generated'
import { COLLECTION_KEY_SCHEME, LABEL_MAX_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Creates a collection key in two steps: name it, then copy it. The second step is the only
 * time the secret exists outside its hash, so the dialog does not close on an outside click.
 */
const props = defineProps<{ site: Site }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const name = ref('')
const created = ref<CollectionKeyCreated | null>(null)

const create = useMutation({
  mutationFn: (args: CollectionKeysCreateArgs) => sitesResource.createCollectionKey(args),
  onSuccess: (result) => {
    created.value = result
    void queryClient.invalidateQueries({ queryKey: queryKeys.collectionKeys(props.site.id) })
  },
})

watch(open, (value) => {
  if (value) return
  name.value = ''
  created.value = null
  create.reset()
})

const error = computed(() => (create.error.value instanceof Error ? create.error.value : null))
const submit = () => create.mutate({ site: props.site.id, name: name.value })
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="max-w-lg"
      @interact-outside="(event) => created && event.preventDefault()"
    >
      <template v-if="created">
        <DialogHeaderCombined
          :title="t('collectionKeys.createdTitle')"
          :description="t('collectionKeys.createdDescription', { scheme: COLLECTION_KEY_SCHEME })"
        />
        <SecretReveal
          id="collection-key-secret"
          :label="t('collectionKeys.secret')"
          :secret="created.secret"
        />
        <div class="flex justify-end">
          <Button
            variant="primary"
            @click="open = false"
          >
            {{ t('actions.done') }}
          </Button>
        </div>
      </template>
      <template v-else>
        <DialogHeaderCombined
          :title="t('collectionKeys.create')"
          :description="t('collectionKeys.createDescription')"
        />
        <form
          class="grid gap-5"
          novalidate
          @submit.prevent="submit"
        >
          <FormField
            id="collection-key-name"
            v-model="name"
            autocomplete="off"
            :label="t('collectionKeys.name')"
            :description="t('collectionKeys.nameHint', { max: LABEL_MAX_CHARS })"
            :error="fieldMessage(error, 'name')"
            required
          />
          <CommandAlert
            :shown="['name']"
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
              :loading="create.isPending.value"
              :disabled="name.trim() === ''"
            >
              {{ t('collectionKeys.create') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
