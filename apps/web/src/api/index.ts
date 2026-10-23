import { runtimeConfig } from '~/services/runtime-config'

import { ApiClient } from './client'

export { ApiError, NetworkError } from './client'

/** The application's single transport, aimed at the API base from the runtime config. */
export const api = new ApiClient(() => runtimeConfig.api_base)
