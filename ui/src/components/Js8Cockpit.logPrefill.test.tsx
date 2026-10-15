// @vitest-environment jsdom
//
// THE JS8 LOG STRIP TAKES JS8Call'S LOG-DIALOG PREFILL (mainwindow.cpp:3858-3905): the selected
// station's SNR at this station is the report SENT (it went in as the report received, which is
// what a CW decoder's read of the other side's report is, not what an SNR is), the contact starts
// when the call was selected, and the frequency logged is the dial plus the TX offset.
//
// Rendered through the REAL cockpit and the REAL log strip, and asserted by value on the record
// handed to `logQso`, never on a class name.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor, screen } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import type { AppSnapshot, Js8State, LoggedQso } from '../types'
import type { PanelLayoutApi, Js8PanelId } from '../features/panelState'
import type { LogQuestion } from '../features/logAnswers'

vi.setConfig({ testTimeout: 20_000 })

const js8Fixture = (): Js8State => ({
  speed: 'normal',
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
  armed: { autoreply: false, cq: false, relay: false, hbAck: false, hb: false },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [
    { call: 'W0IND', grid: 'EN52', snrDb: -8, freqHz: 1508, speed: 'normal', lastMs: 1_757_000_015_000, lastHb: false, lastCq: true, storedMsgs: 0 },
  ],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
}) as unknown as Js8State
let js8 = js8Fixture()

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    getJs8State: vi.fn(async () => js8),
    js8Enter: vi.fn(async () => js8),
    js8Composer: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
    logQso: vi.fn(async () => ({})),
    // The log's answers, over an empty log, as the engine gives them.
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, [])),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator?: ReactNode }) => <header className="cockpit-header">{p.modeIndicator}</header>,
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  radio: {
    dialMhz: 14.078,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1508,
    txOffsetHz: 1600,
    txLevel: 0.5,
  },
  hunt: null,
} as unknown as AppSnapshot

function panels(): PanelLayoutApi<Js8PanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: () => 'docked',
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  js8 = js8Fixture()
  window.localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the JS8 log strip takes JS8Call’s log-dialog prefill', () => {
  it('their SNR is the report sent, the start is the selection, the frequency is dial + TX offset', async () => {
    const SELECTED = 1_760_000_000_000
    const LOGGED = SELECTED + 7 * 60_000
    const now = vi.spyOn(Date, 'now').mockReturnValue(SELECTED)
    render(<Js8Cockpit snap={snap} panels={panels()} />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    const call = await waitFor(() => {
      const b = Array.from(document.querySelectorAll<HTMLButtonElement>('.js8-station-call')).find(
        (x) => x.textContent === 'W0IND',
      )
      expect(b, 'the roster row for W0IND').toBeTruthy()
      return b!
    })
    fireEvent.click(call)
    await waitFor(() =>
      expect((screen.getByPlaceholderText('Call') as HTMLInputElement).value, 'the strip took the call').toBe('W0IND'),
    )
    expect((screen.getByLabelText('Sent') as HTMLInputElement).value, 'their SNR, as the report sent').toBe('-08')
    expect((screen.getByLabelText('Rcvd') as HTMLInputElement).value, 'the received report is untouched').toBe('599')

    now.mockReturnValue(LOGGED)
    fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    const { logQso } = await import('../api')
    await waitFor(() => expect(logQso).toHaveBeenCalledTimes(1))
    const rec = vi.mocked(logQso).mock.calls[0][0] as LoggedQso
    expect({
      call: rec.call,
      rstSent: rec.rstSent,
      rstRcvd: rec.rstRcvd,
      freqMhz: rec.freqMhz,
      whenUnix: rec.whenUnix,
      timeOffUnix: rec.timeOffUnix,
      mode: rec.mode,
    }).toEqual({
      call: 'W0IND',
      rstSent: '-08',
      rstRcvd: '599',
      freqMhz: 14.0796,
      whenUnix: SELECTED / 1000,
      timeOffUnix: LOGGED / 1000,
      mode: 'JS8',
    })
  })

  it('the report sent follows their latest SNR until the operator types one', async () => {
    render(<Js8Cockpit snap={snap} panels={panels()} />)
    const call = await waitFor(() => {
      const b = Array.from(document.querySelectorAll<HTMLButtonElement>('.js8-station-call')).find(
        (x) => x.textContent === 'W0IND',
      )
      expect(b).toBeTruthy()
      return b!
    })
    fireEvent.click(call)
    const sent = () => (screen.getByLabelText('Sent') as HTMLInputElement).value
    await waitFor(() => expect(sent()).toBe('-08'))
    js8 = { ...js8, stations: [{ ...js8.stations[0], snrDb: -12 }] }
    await waitFor(() => expect(sent(), 'their newer SNR').toBe('-12'), { timeout: 3000 })
    fireEvent.change(screen.getByLabelText('Sent'), { target: { value: '-05' } })
    js8 = { ...js8, stations: [{ ...js8.stations[0], snrDb: -15 }] }
    await act(async () => {
      await new Promise((r) => setTimeout(r, 1200))
    })
    expect(sent(), 'never over a typed report').toBe('-05')
  })
})
