// @vitest-environment jsdom
//
// THE STATS PANE KEEPS ONE HEIGHT (CallStatsPane, ONE HEIGHT), as the cascade resolves it over the
// real sheets (the testkit's importance → specificity → source order, jsdom's own selector engine).
// The pane draws the same lines in every state, and CallStatsPane.test.tsx counts them; a line of a
// grid is as tall as the tallest slot mark in it, so every slot's mark takes one height, its margins
// included, whatever the slot says. jsdom lays nothing out, so this holds the rule, and Chrome
// measured what it paints: with a no-credit dash 2 px taller than the other marks, an answer with a
// DXCC grid made the pane 2 px taller than no answer, and RTTY's and PSK's call field, in the column
// above it, moved by that much on every contact.
import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { loadSheets, css } from '../cssCascade.testkit'
import { CallStatsPane } from './CallStatsPane'
import type { CallStats, QslChannels, StatsSlot } from '../types'
import { getCallStats } from '../api'

vi.mock('../api', () => ({ getCallStats: vi.fn() }))

beforeAll(() => loadSheets())
afterEach(cleanup)

const BANDS = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m']
const NONE: QslChannels = { card: false, lotw: false, eqsl: false, eqslAg: false, qrz: false }
const KINDS: StatsSlot[] = ['unworked', 'worked', 'confirmed', 'noCredit']

/** A length the sheet gives a mark, as em and px: `0.8em`, `2px`, `0`, `calc(0.4em + 1px)`. */
function length(v: string | null, what: string): [number, number] {
  expect(v, `${what} is not declared`).toBeTruthy()
  const terms = v!.trim().replace(/^calc\((.*)\)$/, '$1').replace(/\s+/g, '').match(/[+-]?[^+-]+/g) ?? []
  let [em, px] = [0, 0]
  for (const term of terms) {
    const m = /^([+-]?\d*\.?\d+)(em|px)?$/.exec(term)
    expect(m && (m[2] != null || Number(m[1]) === 0), `${what}: a length this guard cannot read, ${v}`).toBeTruthy()
    if (m![2] === 'em') em += Number(m![1])
    else px += Number(m![1])
  }
  return [em, px]
}

/** How much height a mark takes in its line: its height and its margins above and below. */
function extent(mark: Element): [number, number] {
  expect(css(mark, 'display'), 'a mark is a block').toBe('block')
  expect(css(mark, 'box-sizing'), 'a mark’s border is inside its height').toBe('border-box')
  const [h, top, bottom] = ['height', 'margin-top', 'margin-bottom'].map((p) => length(css(mark, p), p))
  return [h[0] + top[0] + bottom[0], h[1] + top[1] + bottom[1]].map((n) => Math.round(n * 1e6) / 1e6) as [number, number]
}

describe('the Stats pane keeps one height', () => {
  it('draws every slot’s mark in one height, margins included, so no answer makes a line taller', async () => {
    // Every kind of slot an answer draws in the DXCC grid; no WAS or WAZ grid, so their slots draw empty.
    const answer: CallStats = {
      call: 'W1AW',
      bands: BANDS,
      modes: ['CW', 'Phone', 'Digital'],
      newOne: false,
      newBand: false,
      newMode: false,
      dxcc: { label: 'United States', code: 291, rows: [0, 1, 2].map((r) => BANDS.map((_, i) => KINDS[(i + r) % 4])) },
      wasFamily: true,
      was: null,
      waz: null,
      countryQsl: NONE,
      callQsl: NONE,
    }
    vi.mocked(getCallStats).mockResolvedValue(answer)
    render(<CallStatsPane call="W1AW" band="20m" mode="CW" />)
    await screen.findByText('United States · DXCC 291')
    const slots = [...document.querySelectorAll('td.cstats-slot')]
    const states = [...new Set(slots.map((td) => td.getAttribute('data-slot')!))].sort()
    expect(states, 'premise: the pane draws every kind of slot').toEqual(['confirmed', 'noCredit', 'none', 'unworked', 'worked'])
    const of = (state: string) => extent(slots.find((td) => td.getAttribute('data-slot') === state)!.querySelector('.cstats-mark')!)
    for (const state of states) expect(of(state), `a ${state} slot’s mark`).toEqual(of('unworked'))
  })
})
