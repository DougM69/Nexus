// @vitest-environment jsdom
//
// CW SPEED MATCH IN THE CW COCKPIT. With the switch on, an F-key that answers a station (a
// contest step other than the CQ, or any key whose message does not start with CQ) goes to the
// engine marked as an answer, which keys it at the station's measured speed; the CQ goes
// unmarked. With the switch off every send is exactly what it was. The switch sits in the contest
// strip while a contest runs.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, FieldDayStatus } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "keys CQ TEST in another contest, CQ FD in Field Day…", takes
// 0.29 s and 0.28 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

/** What the mount-time `getSettings` resolves with. Mutable per test, like `decodeState` —
 *  the cockpit reads `rigModel` from here to decide whether the CAT-keying caution applies. */
const settingsState = {
  macros: { cwProfiles: [] as unknown[], activeCwProfile: 0 },
  rigModel: 0,
}
/** The backend's "CAT CW keying is unproven on this model" list. Empty = rule unread. */
let unprovenModels: number[] = []

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => settingsState),
  getCatCwUnprovenRigModels: vi.fn(async () => unprovenModels),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  sendCwArmed: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(async () => decodeState),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => null),
  previewCw: vi.fn(async (t: string) => t),
  pointRotatorAtCall: vi.fn(async () => 0),
  setRigFunc: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  setTune: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
}))

vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
// The stub reports `titled` so this suite can see the ONE prop that is a placement decision
// rather than log behaviour: whether the strip draws its own heading under a frame head that
// already says LOG. The strip's own half is in LogEntry.density.test.tsx.
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { titled?: boolean }) => (
    <div data-testid="log-stub" data-titled={String(p.titled ?? true)} />
  ),
}))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => {
  decodeState.sent = []
  decodeState.keyerError = null
  settingsState.macros.cwProfiles = []
  settingsState.rigModel = 0
  unprovenModels = []
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'CW',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      cwWpm: 22,
      cwKeyer: 'cat',
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

async function renderCockpit(props: Partial<Parameters<typeof CwCockpit>[0]> = {}) {
  const r = render(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} {...props} />)
  // Let the mount-time getSettings / cwDecode / previewCw promises land.
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}


const ILQP = { event: 'ilqp', running: false } as unknown as FieldDayStatus

const fkey = (i: number) => [...document.querySelectorAll('.cw-macro')][i] as HTMLElement

async function api() {
  return (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
}

describe('CW speed match', () => {
  it('marks a contest answer (F4 my call, F2 his call and exchange) as one, and not the CQ', async () => {
    const a = await api()
    a.sendCw.mockClear()
    await renderCockpit({ fieldDay: ILQP, speedMatch: { on: true, onSwitch: () => {} } })
    await act(async () => {
      fireEvent.click(fkey(3)) // F4 my call
      fireEvent.click(fkey(1)) // F2 his call and exchange
      fireEvent.click(fkey(0)) // F1 CQ
    })
    expect(a.sendCw.mock.calls).toEqual([
      ['{MYCALL}', true],
      ['! {EXCH}', true], // the QSO party's exchange has no report
      ['CQ TEST DE {MYCALL} {MYCALL} K'],
    ])
  })

  it('marks an everyday key by its message: a CQ is not an answer, a reply is', async () => {
    const a = await api()
    a.sendCw.mockClear()
    await renderCockpit({ speedMatch: { on: true, onSwitch: () => {} } })
    await act(async () => {
      fireEvent.click(fkey(0)) // F1 CQ CQ DE …
      fireEvent.click(fkey(1)) // F2, the everyday reply
    })
    expect(a.sendCw.mock.calls[0]).toEqual(['CQ CQ DE {MYCALL} {MYCALL} K'])
    expect(a.sendCw.mock.calls[1][1]).toBe(true)
  })

  it('with the switch off, sends exactly what it sent before', async () => {
    const a = await api()
    a.sendCw.mockClear()
    await renderCockpit({ fieldDay: ILQP, speedMatch: { on: false, onSwitch: () => {} } })
    await act(async () => {
      fireEvent.click(fkey(3))
      fireEvent.click(fkey(0))
    })
    expect(a.sendCw.mock.calls).toEqual([['{MYCALL}'], ['CQ TEST DE {MYCALL} {MYCALL} K']])
  })

  it('shows its switch in the contest strip, and only while a contest runs', async () => {
    const onSwitch = vi.fn()
    await renderCockpit({ fieldDay: ILQP, speedMatch: { on: false, onSwitch } })
    const sw = document.querySelector('[role="switch"][aria-label="CW speed match"]') as HTMLElement
    expect(sw).not.toBeNull()
    expect(sw.getAttribute('aria-checked')).toBe('false')
    fireEvent.click(sw)
    expect(onSwitch).toHaveBeenCalledWith(true)
    cleanup()
    await renderCockpit({ speedMatch: { on: true, onSwitch } })
    expect(document.querySelector('[aria-label="CW speed match"]')).toBeNull()
  })
})
