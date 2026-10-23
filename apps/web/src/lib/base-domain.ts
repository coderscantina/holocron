/**
 * Two-label public suffixes under which a registrable name has three labels
 * (`shop.example.co.uk` belongs to `example.co.uk`, not to `co.uk`). A short list of the suffixes
 * sites here are likely to use, not the full public suffix list.
 */
const twoLabelSuffixes: ReadonlySet<string> = new Set([
  'co.uk', 'org.uk', 'ac.uk', 'gov.uk', 'me.uk', 'ltd.uk', 'plc.uk',
  'com.au', 'net.au', 'org.au', 'edu.au', 'gov.au',
  'co.nz', 'org.nz', 'net.nz',
  'co.at', 'or.at', 'gv.at', 'ac.at',
  'co.jp', 'or.jp', 'ne.jp', 'ac.jp',
  'co.za', 'org.za',
  'co.in', 'net.in', 'org.in',
  'com.br', 'net.br', 'org.br',
  'com.mx', 'com.ar', 'com.tr', 'com.cn', 'com.hk', 'com.sg', 'com.tw',
  'co.kr', 'co.il', 'co.id', 'co.th',
])

const isIpLike = (host: string) => /^\d{1,3}(\.\d{1,3}){3}$/.test(host) || host.includes(':')

/**
 * The name a site's domain is grouped under: its last two labels, or the last three under a
 * two-label public suffix. IP addresses and single-label hosts are their own group. The result is
 * lowercase; internationalized labels pass through unchanged, in Unicode or `xn--` form.
 */
export function baseDomain(domain: string): string {
  const host = domain.trim().toLowerCase().replace(/\.$/, '')
  if (isIpLike(host)) return host
  const labels = host.split('.')
  if (labels.length <= 2) return host
  const keep = twoLabelSuffixes.has(labels.slice(-2).join('.')) ? 3 : 2
  return labels.slice(-keep).join('.')
}

export interface DomainGroup<T> {
  baseDomain: string
  sites: T[]
}

/** Groups sites by base domain, groups in alphabetical order and sites in the order given. */
export function groupByBaseDomain<T extends { domain: string }>(sites: readonly T[]): DomainGroup<T>[] {
  const groups = new Map<string, T[]>()
  for (const site of sites) {
    const key = baseDomain(site.domain)
    const group = groups.get(key)
    if (group) group.push(site)
    else groups.set(key, [site])
  }
  return [...groups]
    .map(([base, members]) => ({ baseDomain: base, sites: members }))
    .sort((a, b) => a.baseDomain.localeCompare(b.baseDomain))
}
