import { describe, expect, it } from 'vitest'

import { baseDomain, groupByBaseDomain } from '~/lib/base-domain'

describe('baseDomain', () => {
  it('keeps the last two labels of a subdomain', () => {
    expect(baseDomain('shop.roadhouse.test')).toBe('roadhouse.test')
    expect(baseDomain('a.b.c.example.com')).toBe('example.com')
    expect(baseDomain('Example.COM.')).toBe('example.com')
  })

  it('keeps three labels under a two-label public suffix', () => {
    expect(baseDomain('shop.example.co.uk')).toBe('example.co.uk')
    expect(baseDomain('example.com.au')).toBe('example.com.au')
    expect(baseDomain('www.stadt.gv.at')).toBe('stadt.gv.at')
    expect(baseDomain('verein.or.at')).toBe('verein.or.at')
  })

  it('leaves IP-like and single-label hosts alone', () => {
    expect(baseDomain('192.168.0.12')).toBe('192.168.0.12')
    expect(baseDomain('[::1]')).toBe('[::1]')
    expect(baseDomain('localhost')).toBe('localhost')
  })

  it('passes internationalized names through', () => {
    expect(baseDomain('shop.bücher.de')).toBe('bücher.de')
    expect(baseDomain('www.xn--bcher-kva.example.xn--p1ai')).toBe('example.xn--p1ai')
    expect(baseDomain('blog.пример.рф')).toBe('пример.рф')
  })
})

describe('groupByBaseDomain', () => {
  it('orders groups by base domain and keeps site order inside a group', () => {
    const sites = [
      { id: 1, domain: 'shop.roadhouse.test' },
      { id: 2, domain: 'acme.com' },
      { id: 3, domain: 'roadhouse.test' },
    ]
    expect(groupByBaseDomain(sites).map((group) => [group.baseDomain, group.sites.map((site) => site.id)])).toEqual([
      ['acme.com', [2]],
      ['roadhouse.test', [1, 3]],
    ])
  })
})
