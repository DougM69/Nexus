// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { alcBar, TxMeters } from './TxMeters'
import type { RadioStatus } from '../types'

afterEach(cleanup)

// The ALC bar's colours sit where the radio's own ALC zone ends. On the FT-991A, FT-710 and
// FTDX10 the bar is the radio's own (0 to 255 over the whole bar) and the zone ends at half
// scale; everywhere else 1.0 is the top of the zone. The colours keep their place inside the
// zone: amber from 80 % of it, red from 95 %.

describe('the ALC colours follow the radio’s own zone', () => {
  it('a Yaesu bar read on its own scale: green, amber and red by its half-scale zone', () => {
    const zones = [0.1, 0.39, 0.4, 0.47, 0.475, 0.5, 0.6, 0.8, 1.0].map((v) => [v, alcBar(v, 0.5).zone])
    expect(zones).toEqual([
      [0.1, 'ok'],
      [0.39, 'ok'],
      [0.4, 'warn'],
      [0.47, 'warn'],
      [0.475, 'hot'],
      [0.5, 'hot'],
      [0.6, 'hot'],
      [0.8, 'hot'],
      [1.0, 'hot'],
    ])
    // The bar itself stays where the radio's bar is: only the colour moves.
    expect(alcBar(0.6, 0.5)).toMatchObject({ frac: 0.6, value: '60%' })
  })

  it('every other radio is as before: 1.0 is the top of its zone', () => {
    const zones = [0.5, 0.79, 0.8, 0.94, 0.95, 1.0].map((v) => [v, alcBar(v).zone])
    expect(zones).toEqual([
      [0.5, 'ok'],
      [0.79, 'ok'],
      [0.8, 'warn'],
      [0.94, 'warn'],
      [0.95, 'hot'],
      [1.0, 'hot'],
    ])
  })

  it('the meter paints the zone the reading carries', () => {
    const alcFill = (radio: Partial<RadioStatus>) => {
      const rows = render(<TxMeters radio={{ rigKeyed: true, ...radio } as unknown as RadioStatus} />)
        .container.querySelectorAll('.ph-txmeter')
      const row = [...rows].find((r) => r.querySelector('.ph-txmeter-label')?.textContent === 'ALC')
      const fill = row?.querySelector('.ph-txmeter-fill') as HTMLElement
      const painted = fill.getAttribute('style') ?? ''
      const width = fill.style.width
      cleanup()
      return {
        width,
        colour: ['--state-good', '--alert-warning', '--state-weak'].find((v) => painted.includes(v)),
      }
    }
    expect(alcFill({ txAlc: 0.6, txAlcZone: 0.5 })).toEqual({ width: '60%', colour: '--state-weak' })
    expect(alcFill({ txAlc: 0.45, txAlcZone: 0.5 })).toEqual({ width: '45%', colour: '--alert-warning' })
    expect(alcFill({ txAlc: 0.3, txAlcZone: 0.5 })).toEqual({ width: '30%', colour: '--state-good' })
    // Control: the same 60 % from a radio whose 1.0 is the zone top is green, as it always was.
    expect(alcFill({ txAlc: 0.6 })).toEqual({ width: '60%', colour: '--state-good' })
    expect(alcFill({ txAlc: 0.6, txAlcZone: null })).toEqual({ width: '60%', colour: '--state-good' })
  })
})
