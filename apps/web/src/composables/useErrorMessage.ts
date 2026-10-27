import { ApiError, NetworkError } from '~/api'
import type { ErrorCode, FieldErrorCode } from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/** Exhaustive: a new generated code is a compile error until it has copy. */
const errorMessages: Record<ErrorCode, MessageKey> = {
  unauthenticated: 'errors.unauthenticated',
  invalid_credentials: 'errors.invalid_credentials',
  second_factor_required: 'errors.second_factor_required',
  second_factor_invalid: 'errors.second_factor_invalid',
  second_factor_enrollment_required: 'errors.second_factor_enrollment_required',
  reauthentication_required: 'errors.reauthentication_required',
  not_found: 'errors.not_found',
  forbidden: 'errors.forbidden',
  operation_not_granted: 'errors.operation_not_granted',
  grant_exceeds_principal: 'errors.grant_exceeds_principal',
  share_scope_exceeded: 'errors.share_scope_exceeded',
  origin_rejected: 'errors.origin_rejected',
  stale_revision: 'errors.stale_revision',
  idempotency_conflict: 'errors.idempotency_conflict',
  user_owns_resources: 'errors.user_owns_resources',
  last_install_admin: 'errors.last_install_admin',
  saved_replay_budget_exceeded: 'errors.saved_replay_budget_exceeded',
  malformed_request: 'errors.malformed_request',
  invalid_arguments: 'errors.invalid_arguments',
  duplicate_predicate_id: 'errors.duplicate_predicate_id',
  filter_too_deep: 'errors.filter_too_deep',
  too_many_conditions: 'errors.too_many_conditions',
  page_out_of_range: 'errors.page_out_of_range',
  report_missing: 'errors.report_missing',
  report_invalid: 'errors.report_invalid',
  report_field_unsupported: 'errors.report_field_unsupported',
  report_version_unsupported: 'errors.report_version_unsupported',
  report_too_large: 'errors.report_too_large',
  compare_unsupported: 'errors.compare_unsupported',
  payload_too_large: 'errors.payload_too_large',
  rate_limited: 'errors.rate_limited',
  query_timeout: 'errors.query_timeout',
  setup_required: 'errors.setup_required',
  setup_token_invalid: 'errors.setup_token_invalid',
  import_in_progress: 'errors.import_in_progress',
  internal: 'errors.internal',
}

const fieldMessages: Record<FieldErrorCode, MessageKey> = {
  required: 'fieldErrors.required',
  too_short: 'fieldErrors.too_short',
  too_long: 'fieldErrors.too_long',
  too_many: 'fieldErrors.too_many',
  invalid_format: 'fieldErrors.invalid_format',
  invalid_value: 'fieldErrors.invalid_value',
}

export const errorMessageKey = (code: ErrorCode): MessageKey => errorMessages[code]

type Translate = (key: MessageKey, named?: Record<string, unknown>) => string

/**
 * Copy for any thrown value: API codes, network loss, or the unexpected. A refused origin names
 * the address to open when the server sent one (it is set only with a public URL); otherwise
 * the generic hint stays.
 */
export function messageFor(error: unknown, t: Translate): string {
  if (error instanceof ApiError) {
    const origin = error.body.expected_origin
    if (error.code === 'origin_rejected' && origin) return t('errors.origin_rejected_at', { origin })
    return t(errorMessages[error.code])
  }
  if (error instanceof NetworkError) return t('errors.network')
  return t('errors.internal')
}

/** Translated copy for any thrown value. */
export function useErrorMessage() {
  const { t } = useI18n()

  const message = (error: unknown): string => messageFor(error, t)

  /** The first problem per field, translated, for `invalid_arguments`. */
  const fieldMessage = (error: unknown, field: string): string | null => {
    if (!(error instanceof ApiError)) return null
    const first = error.body.errors?.[field]?.[0]
    return first ? t(fieldMessages[first.code], { limit: first.limit ?? '' }) : null
  }

  return { message, fieldMessage }
}
