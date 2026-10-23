import type { LocationQuery } from 'vue-router'

/** The `redirect` query value when it is a same-app path; an absolute URL would send the person elsewhere. */
export function safeRedirect(query: LocationQuery): string {
  const redirect = query.redirect
  return typeof redirect === 'string' && redirect.startsWith('/') && !redirect.startsWith('//') ? redirect : '/'
}
