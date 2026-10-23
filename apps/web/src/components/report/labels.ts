import type { SessionColumn } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/** Exhaustive labels of the session list columns (0004 Report definitions). */
export const sessionColumnLabels = {
  session_id: 'reports.sessions.columns.session_id',
  identity_kind: 'reports.sessions.columns.identity_kind',
  start: 'reports.sessions.columns.start',
  end: 'reports.sessions.columns.end',
  span: 'reports.sessions.columns.span',
  pageviews: 'reports.sessions.columns.pageviews',
  custom_events: 'reports.sessions.columns.custom_events',
  entry_path: 'reports.sessions.columns.entry_path',
  exit_path: 'reports.sessions.columns.exit_path',
  referrer_domain: 'reports.sessions.columns.referrer_domain',
  channel: 'reports.sessions.columns.channel',
  utm_source: 'reports.sessions.columns.utm_source',
  utm_campaign: 'reports.sessions.columns.utm_campaign',
  country: 'reports.sessions.columns.country',
  city: 'reports.sessions.columns.city',
  device: 'reports.sessions.columns.device',
  browser: 'reports.sessions.columns.browser',
  os: 'reports.sessions.columns.os',
} as const satisfies Record<SessionColumn, MessageKey>
