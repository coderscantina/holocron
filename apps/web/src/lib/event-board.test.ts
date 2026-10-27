import { describe, expect, it } from 'vitest'

import { SHADE_FLOOR, shades } from '~/lib/event-board'

describe('shades', () => {
  it('scales each row from zero to its own busiest bucket', () => {
    expect(shades([0, 5, 10])).toEqual([SHADE_FLOOR, SHADE_FLOOR + (1 - SHADE_FLOOR) * 0.25, 1])
    expect(shades([0, 0])).toEqual([SHADE_FLOOR, SHADE_FLOOR])
  })
})
