import { icons as lucideIcons } from '@iconify-json/lucide'
import { addCollection } from '@iconify/vue'
import { createApp } from 'vue'

import App from '~/App.vue'
import { appConfig } from '~/lib/app-config'
import { addBrandIcons } from '~/lib/brand-icons'
import { installAuthHandler } from '~/plugins/auth'
import { installI18n } from '~/plugins/i18n'
import { installVueQuery } from '~/plugins/vue-query'
import { router } from '~/router'
import { loadRuntimeConfig } from '~/services/runtime-config'

import '~/assets/css/app.css'

// The bundled set renders icons locally, with no request to the Iconify API. It rides in its
// own long-cached chunk, as in Kit.
addCollection(lucideIcons)
addBrandIcons()

document.documentElement.dataset.density = appConfig.density

await loadRuntimeConfig()

const app = createApp(App)
installI18n(app)
installVueQuery(app)
installAuthHandler()
app.use(router)
app.mount('#app')
