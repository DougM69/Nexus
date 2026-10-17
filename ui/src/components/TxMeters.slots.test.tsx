// @vitest-environment jsdom
//
// EVERY METER HAS ITS SLOT FROM THE FIRST MOMENT OF THE OVER (operator, 2026-10-17).
//
// The Operate strip's `.cq-telemetry` cell used to show only the meters that had read, in an
// `auto-fit` grid. The keyed poll reads one meter per 150 ms (SWR → ALC → Po → COMP), so the
// first over after a mount showed SWR alone for a moment, and `auto-fit` collapsed the empty
// tracks: that lone bar spanned the whole 21em cell, then shrank to a third as the others
// arrived. So now, once the rig keys, the cell holds all four slots at fixed, equal widths,
// each EMPTY until its meter reads, and nothing in the cell moves when a reading lands.
//
// jsdom never lays out, so this asserts what the browser is handed: the slots the cell renders
// at each step of an over, and the grid rule that wins the cascade for the rendered cell (the
// real selector engine over the real sheets, cssCascade.testkit). Slot widths themselves are
// measured in a real browser, not here.
import { describe, it, expect, afterEach, beforeAll } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { TxMeters } from './TxMeters'
import type { RadioStatus } from '../types'
import { loadSheets, css } from '../cssCascade.testkit'

beforeAll(() => loadSheets())
afterEach(cleanup)

const radio = (over: Record<string, unknown> = {}) => ({ ...over }) as unknown as RadioStatus
const keyed = (over: Record<string, unknown> = {}) => radio({ rigKeyed: true, ...over })

const panel = () => document.querySelector('.ph-txmeters')!
const rows = () => [...document.querySelectorAll('.ph-txmeter')]
const labels = () => rows().map((r) => r.querySelector('.ph-txmeter-label')?.textContent)
const values = () => rows().map((r) => r.querySelector('.ph-txmeter-value')?.textContent)
const fills = () => rows().map((r) => (r.querySelector('.ph-txmeter-fill') as HTMLElement).style.width)
/** Everything about the cell's shape that a reading arriving could change: which slots, in what
 *  order, whether a ⊘ or the hint takes room in it. Values and fill widths are excluded: those
 *  are the readings themselves. */
const shape = () => ({
  slots: rows().map((r) => r.querySelector('.ph-txmeter-label')?.textContent?.replace('?', '')),
  unavail: document.querySelectorAll('.ph-unavail').length,
  hint: panel().querySelector('.ph-txmeters-hint') != null,
})

/** The cell as Operate mounts it: the inline meters inside the fixed-width telemetry cell. */
const cell = (r: RadioStatus) => (
  <div className="cq-telemetry">
    <TxMeters radio={r} inline />
  </div>
)

describe('the Operate cell gives every meter its slot from the first moment of the over', () => {
  it('keyed with nothing read yet: four empty slots, no hint', () => {
    render(cell(keyed()))
    expect(labels(), 'the cell must not wait for a reading to give a meter its slot').toEqual([
      'SWR',
      'ALC',
      'PO',
      'COMP',
    ])
    expect(values()).toEqual(['—', '—', '—', '—'])
    expect(fills(), 'an empty slot is empty, not a full bar').toEqual(['0%', '0%', '0%', '0%'])
    expect(panel().querySelector('.ph-txmeters-hint'), 'the hint would take a line and then vanish').toBeNull()
  })

  it('the cell keeps one shape while the round-robin brings the readings in', () => {
    const steps = [
      keyed(),
      keyed({ txSwr: 1.0, swrScaleVerified: true }),
      keyed({ txSwr: 1.0, txAlc: 0.2, swrScaleVerified: true }),
      keyed({ txSwr: 1.0, txAlc: 0.2, txPoW: 88, swrScaleVerified: true }),
      keyed({ txSwr: 1.0, txAlc: 0.2, txPoW: 88, txCompDb: 6, swrScaleVerified: true }),
    ]
    const r = render(cell(steps[0]))
    const first = shape()
    expect(first).toEqual({ slots: ['SWR', 'ALC', 'PO', 'COMP'], unavail: 0, hint: false })
    for (const [i, s] of steps.entries()) {
      r.rerender(cell(s))
      expect(shape(), `step ${i}: a reading arriving changed the cell`).toEqual(first)
    }
    expect(values()).toEqual(['1.0:1', '20%', '88 W', '6 dB'])
  })

  it('SWR alone is one slot of four, not the whole cell', () => {
    // Screenshot B of the report: the lone SWR bar ~2 inches long. It is one slot now.
    render(cell(keyed({ txSwr: 1.0, swrScaleVerified: true })))
    expect(rows()).toHaveLength(4)
    expect(values()).toEqual(['1.0:1', '—', '—', '—'])
  })

  it('no ⊘ while the first over is still reading: a meter not yet polled is not a missing one', () => {
    const r = render(cell(keyed({ txSwr: 1.2, swrScaleVerified: true })))
    expect(document.querySelectorAll('.ph-unavail'), 'ALC/PO/COMP written off 150 ms into the over').toHaveLength(0)
    // After the over the verdict is due: the rig keyed and these stayed silent.
    r.rerender(cell(radio()))
    expect(document.querySelectorAll('.ph-unavail')).toHaveLength(3)
    // …and it stays put through the next over, so the cell does not change shape per over.
    r.rerender(cell(keyed({ txSwr: 1.3, swrScaleVerified: true })))
    expect(document.querySelectorAll('.ph-unavail')).toHaveLength(3)
  })

  it('keeps the four slots between overs, dimmed', () => {
    const r = render(cell(keyed({ txPoW: 42 })))
    r.rerender(cell(radio()))
    expect(labels()).toEqual(['SWR', 'ALC', 'PO', 'COMP'])
    expect(panel().classList.contains('idle')).toBe(true)
  })

  it('before the rig has ever keyed it is still the hint alone (the 1280×800 fold)', () => {
    render(cell(radio()))
    expect(rows()).toHaveLength(0)
    expect(panel().querySelector('.ph-txmeters-hint')).not.toBeNull()
  })
})

describe('the slot widths are fixed by the grid, not by how many meters have read', () => {
  it('the rendered cell wins a fixed two-track grid', () => {
    render(cell(keyed({ txSwr: 1.0, swrScaleVerified: true })))
    const cols = css(panel(), 'grid-template-columns')
    // By value. `auto-fit`/`auto-fill` is the defect: it collapses the tracks of slots that
    // are not there, which is how one bar got the whole cell.
    expect(cols, 'the inline cell lost its fixed grid').toBe('repeat(2, minmax(0, 1fr))')
    // Four slots fill two whole rows, so no slot is ever alone on a row of its own.
    expect(rows().length % 2).toBe(0)
    // And no slot spans tracks of its own accord.
    for (const r of rows()) expect(css(r, 'grid-column'), 'a slot spans more than its track').toBeNull()
  })

  it('the dock keeps its own grid (the change is the inline cell only)', () => {
    render(<TxMeters radio={keyed({ txSwr: 1.0, swrScaleVerified: true })} pinned />)
    expect(css(panel(), 'grid-template-columns')).toBe('repeat(auto-fit, minmax(150px, 1fr))')
    expect(labels()).toEqual(['SWR', 'ALC', 'PO', 'COMP'])
  })
})
