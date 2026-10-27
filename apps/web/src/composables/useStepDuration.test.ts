import { afterEach, describe, expect, it } from 'vitest'

import { composer } from '~/plugins/i18n'

import { useStepDuration } from './useStepDuration'

describe('useStepDuration', () => {
  afterEach(() => {
    composer.locale.value = 'en'
  })

  it('writes german units as Tg., Std., Min. and Sek.', () => {
    composer.locale.value = 'de'
    const { span } = useStepDuration()
    expect(span(3 * 86_400_000 + 4 * 3_600_000)).toBe('3 Tg. 4 Std.')
    expect(span(159_000)).toBe('2 Min. 39 Sek.')
    expect(span(500)).toBe('unter 1 Sek.')
  })
})
