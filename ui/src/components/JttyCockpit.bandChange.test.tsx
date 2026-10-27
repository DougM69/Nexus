// @vitest-environment jsdom
//
// A BAND CHANGE CLEARS THE JTTY CALL BOX (operator ruling, 2026-10-26): `%H`/`%Q` must not send
// the old band's station on the new one. The engine bumps `bandChangeTick` on every band change,
// by any route; the box empties then and only then.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import type { ReactNode } from 'react'
import { JttyCockpit } from './JttyCockpit'
import type { AppSnapshot, JttyState } from '../types'
import * as api from '../api'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) mod[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  return mod
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (f: () => Promise<unknown>) => f()) }))
// The header's own furniture (readout, digit tuning, the ⊞ menu) is not this file's: it draws the
// slots the cockpit hands it, which is where Rx and F Tol live.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator: ReactNode; bandControl: ReactNode; children?: ReactNode }) => (
    <header className="cockpit-header">
      {p.modeIndicator}
      {p.bandControl}
      {p.children}
    </header>
  ),
}))
/** The waterfall's props as last rendered: the test plays the strip's part (a click, a zoom). */
type WfTarget = 'tx' | 'rx' | 'both'
const wf = vi.hoisted(() => ({
  props: null as null | {
    onTune?: (hz: number, target: 'tx' | 'rx' | 'both') => void
    onViewChange?: (lo: number, hi: number) => void
    rxOffsetHz: number
    txOffsetHz: number
  },
}))
vi.mock('./Waterfall', () => ({
  Waterfall: (p: { onTune?: (hz: number, target: WfTarget) => void; onViewChange?: (lo: number, hi: number) => void; rxOffsetHz: number; txOffsetHz: number }) => {
    wf.props = p
    return <div className="waterfall-wrap" />
  },
}))
vi.mock('./RfScopePane', () => ({ RfScopePane: () => null }))
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { cwLive?: { call: string | null; confirmed: boolean } | null; mode: string }) => (
    <div data-testid="log-stub" data-mode={p.mode}>
      {p.cwLive?.call ?? ''}
    </div>
  ),
}))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.09, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false, txEnabled: false, txAllowed: true },
} as unknown as AppSnapshot

const m = <K extends keyof typeof api>(k: K) => vi.mocked(api[k] as (...a: unknown[]) => Promise<unknown>)
const st: JttyState = { armed: true, rxHz: 1500, ftolHz: 20, lowHz: 200, highHz: 3000, lines: [], heard: [], notice: null } as unknown as JttyState

beforeEach(() => {
  for (const k of ['getJttyState', 'jttyAutoArm', 'jttyArm'] as const) m(k).mockResolvedValue(st)
  m('getLicensedBandPlan').mockResolvedValue([])
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const settle = () =>
  act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
const callBox = () => screen.getByLabelText("The other station's call (%H)") as HTMLInputElement

describe('a band change clears the JTTY Call box', () => {
  it('empties on a band change, and only then', async () => {
    const at = (tick: number, dialMhz: number, band: string) =>
      <JttyCockpit snap={{ ...snap, bandChangeTick: tick, radio: { ...snap.radio, dialMhz, band } }} active onSetTxEnabled={() => {}} />
    const r = render(at(5, 14.09, '20m'))
    await settle()
    fireEvent.change(callBox(), { target: { value: 'K1ABC' } })
    expect(callBox().value, 'control: the call is in the box').toBe('K1ABC')
    r.rerender(at(5, 14.091, '20m'))
    await settle()
    expect(callBox().value, 'no band change: the call stays').toBe('K1ABC')
    r.rerender(at(6, 7.09, '40m'))
    await settle()
    expect(callBox().value).toBe('')
  })
})
