// @vitest-environment jsdom
//
// A BAND CHANGE CLEARS THE JS8 TO BOX (operator ruling, 2026-10-26): the station addressed on the
// old band is not on the new one. The engine bumps `bandChangeTick` on every band change, by any
// route; the box empties, and the directed command picked for that call goes with it (the
// `<SELECTEDCALL>`/SNR values read the box, so they follow). A snapshot with no band change keeps it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, screen, fireEvent } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import type { AppSnapshot, Js8State } from '../types'

// THE BUDGET: the house's 15 s, as the other cockpit suites carry it.
vi.setConfig({ testTimeout: 15_000 })

const base = (): Js8State => ({
  speed: 'slow',
  rxSpeeds: 15,
  txEnabled: false,
  sending: false,
  hbOn: false,
  hbNextAtMs: null,
  hbIntervalMin: 0,
  cqOn: false,
  cqNextAtMs: null,
  cqIntervalMin: 0,
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false },
  notArmed: { autoreply: 'txOff', relay: 'txOff', hbAck: null, hb: null, cq: null },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: base() }

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  const s = () => state.current
  return {
    ...auto,
    getJs8State: vi.fn(async () => s()),
    js8Enter: vi.fn(async () => s()),
    js8Composer: vi.fn(async () => null),
    js8LocatorRefusal: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  state.current = base()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const to = () => screen.getByLabelText('Addressee') as HTMLInputElement

async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

describe('a band change clears the JS8 To box', () => {
  it('the To box empties on a band change, and only then', async () => {
    const r = render(<Js8Cockpit snap={{ ...snap, bandChangeTick: 1 }} />)
    await settle()
    fireEvent.change(to(), { target: { value: 'W1AW' } })
    expect(to().value, 'control: the call is in the box').toBe('W1AW')
    r.rerender(<Js8Cockpit snap={{ ...snap, bandChangeTick: 1, radio: { ...snap.radio, dialMhz: 14.079 } }} />)
    await settle()
    expect(to().value, 'no band change: the call stays').toBe('W1AW')
    r.rerender(<Js8Cockpit snap={{ ...snap, bandChangeTick: 2, radio: { ...snap.radio, dialMhz: 7.078, band: '40m' } }} />)
    await settle()
    expect(to().value).toBe('')
  })
})
