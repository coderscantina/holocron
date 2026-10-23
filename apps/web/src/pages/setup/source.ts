const engines: Record<string, string> = { postgresql: 'PostgreSQL', mysql: 'MySQL' }

/** `umami-3.4.0-postgresql` as "Umami 3.4.0 on PostgreSQL". Anything else stays as it is. */
export function sourceLabel(source: string): string {
  const match = /^umami-([\d.]+)-(\w+)$/.exec(source)
  if (!match) return source
  const [, version, engine] = match
  return `Umami ${version} · ${engines[engine ?? ''] ?? engine}`
}
