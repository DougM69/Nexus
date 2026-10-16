import { expect, it } from 'vitest'
import { spotNeed } from './needs'
import type { SpotRow } from '../types'

const spot = (patch: Partial<SpotRow> = {}): SpotRow => ({
  call: 'JA1ABC', entity: 'Japan', zone: 25, band: '20m', freqMhz: 14.074, mode: 'Digital', submode: 'FT8',
  spotter: 'K1TTT', corroborators: [], ageSecs: 30, comment: '', licensed: true, ...patch,
})

it('turns a Spots row into the Needed-board row the Work path takes', () => {
  expect(spotNeed(spot())).toEqual({
    call: 'JA1ABC', entity: 'Japan', band: '20m', zone: 25, tags: [], priority: 0, headline: '', mode: 'FT8', freqMhz: 14.074,
  })
})

it('keeps FT8 and FT4 as the mode, and falls back to the frequency class for anything else', () => {
  expect(spotNeed(spot({ submode: 'FT4' })).mode).toBe('FT4')
  expect(spotNeed(spot({ submode: 'RTTY' })).mode).toBe('Digital')
  expect(spotNeed(spot({ mode: 'CW', submode: null })).mode).toBe('CW')
})
