import { describe, expect, it } from 'vitest'

import de from './de.json'
import en from './en.json'

/** Every leaf path with its value, e.g. `['nav.overview', 'Overview']`. */
function leaves(tree: object, prefix = ''): Array<[string, unknown]> {
  return Object.entries(tree).flatMap(([key, value]) =>
    typeof value === 'object' && value !== null ? leaves(value, `${prefix}${key}.`) : [[`${prefix}${key}`, value]]
  )
}

const placeholders = (text: unknown) => [...String(text).matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort()

describe('locale catalogs', () => {
  const english = new Map(leaves(en))
  const german = new Map(leaves(de))

  it('define the same keys', () => {
    expect([...german.keys()].sort()).toEqual([...english.keys()].sort())
  })

  it('translate every key with the same placeholders', () => {
    for (const [key, text] of english) {
      const translated = german.get(key)
      expect(typeof translated === 'string' && translated.trim().length > 0, key).toBe(true)
      expect(placeholders(translated), key).toEqual(placeholders(text))
    }
  })
})
