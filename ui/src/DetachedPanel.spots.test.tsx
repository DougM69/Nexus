// @vitest-environment jsdom
//
// THE SPOTS BOARD IN A WINDOW OF ITS OWN (`?panel=spots`), as the Needed board has had one.
//
// The window is the real board (SpotsPanel, the Spots view's own component) with THIS window's paths, and it
// shares them with the dashboard window's Spots box through one object, as the Needed window and the
// dashboard window's Needed box share theirs. A pop-out cannot reach the main window's handlers, so "the
// board's own path" here is this window's: its Needed work applied to the spot as a need, exactly how the
// Spots view works a spot in the main window (`handleWorkSpot = handleWorkNeeded(spotNeed(s))`). THE REAL
// DetachedPanel is mounted, as the Spots window and, where a case is about the shared board, as the
// dashboard window beside it; only the backend is stubbed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, act } from '@testing-library/react'
import type { AppSnapshot, NeedAlert, SpotRow } from './types'
import { t } from './i18n'

const snapshot = {
  mycall: 'KD9TAW', mygrid: 'EN52', mode: 'Normal', activePeer: null, hunt: null, logTick: 1,
  radio: { dialMhz: 14.025, band: '20m', catOk: true, sideband: 'CW', operatingMode: 'cw', transmitting: false, txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' }, stations: [],
} as unknown as AppSnapshot

const spot = (over: Partial<SpotRow>): SpotRow =>
  ({
    call: 'K1CW', entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW',
    submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 30, comment: '', licensed: true,
    spotterLocal: true, ...over,
  }) as SpotRow
const SPOTS: SpotRow[] = [
  spot({}),
  spot({ call: 'JA1FT', entity: 'Japan', band: '17m', freqMhz: 18.104, mode: 'Digital', submode: 'FT4', ageSecs: 45 }),
]
/** Two stations worked today, each still wanted by a need the engine sent on 20 m CW. */
const WORKED: SpotRow[] = [
  spot({ call: 'W7B', freqMhz: 14.03, workedAgoSecs: 600, workedTodayUtc: true }),
  spot({ call: 'W6A', freqMhz: 14.035, workedAgoSecs: 600, workedTodayUtc: true }),
]
const need = (call: string, tags: NeedAlert['tags'], freqMhz: number): NeedAlert =>
  ({ call, entity: 'United States', band: '20m', zone: 5, tags, priority: 50, headline: '', mode: 'CW', freqMhz }) as NeedAlert

const backend = vi.hoisted(() => ({ spots: [] as unknown[], needs: [] as unknown[] }))

const TX_VERBS = [
  'callStation', 'startCq', 'startRunning', 'callCq', 'setChatCq', 'resumeChatCq', 'sendMessage', 'sendCw', 'setPtt',
  'playVoiceMessage', 'setTxEnabled', 'setTune', 'atuTune', 'setBeacon', 'aprsSendBeacon', 'aprsSendMessage',
  'rttyAutoArm', 'rttySend', 'rttySetLatched', 'rttyType', 'rttyAutoCq', 'rttyAutoAnswer', 'pskSend',
  'pskSetLatched', 'pskType', 'js8Send', 'js8SendCommand', 'js8CallCq', 'js8Arm', 'js8CqRepeat', 'sstvSend',
]

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    subscribeSnapshot: vi.fn((cb: (s: AppSnapshot) => void) => {
      cb(snapshot)
      return () => {}
    }),
    selectPeer: vi.fn(async () => snapshot),
    getBandPlan: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getNeedAlerts: vi.fn(async () => backend.needs),
    getSettings: vi.fn(async () => null),
    getAllSpots: vi.fn(async () => backend.spots),
    getOtaSpots: vi.fn(async () => []),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
    getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
    getGettingOut: vi.fn(async () => null),
    getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getDxpedWindows: vi.fn(async () => []),
    getOtaMapSpots: vi.fn(async () => []),
    getContests: vi.fn(async () => []),
    getWindowBehind: vi.fn(async () => ({ supported: false, on: false })),
    workSpot: vi.fn(async () => snapshot),
    setHuntTarget: vi.fn(async () => snapshot),
    clusterSelfSpot: vi.fn(async (comment: string) => ({ freqKhz: 14025, call: 'KD9TAW', comment, cooldownSecs: 60 })),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

import { DetachedPanel } from './DetachedPanel'
import * as api from './api'
import { spotNeed } from './features/needs'
import { pastTheSwitch } from './components/ConnectView.testkit'

// THE BUDGET: DetachedPanel.connectBoards.test.tsx's, for the same mounts (15 s; a hang still fails).
vi.setConfig({ testTimeout: 15_000 })

type Call = [string, unknown[]]
function traffic(): Call[] {
  const out: Array<{ order: number; call: Call }> = []
  for (const [name, fn] of Object.entries(api)) {
    const m = (fn as { mock?: { calls: unknown[][]; invocationCallOrder: number[] } }).mock
    if (!m) continue
    m.calls.forEach((args, i) => out.push({ order: m.invocationCallOrder[i], call: [name, args] }))
  }
  return out.sort((a, b) => a.order - b.order).map((x) => x.call)
}
const writes = (calls: Call[]) => calls.filter(([n]) => !/^(get|ask|read|resolve|preview)/.test(n))
function clearMocks() {
  for (const fn of Object.values(api)) (fn as { mockClear?: () => void }).mockClear?.()
}

/** The Spots window's board, and the dashboard window's Spots box. */
const windowBoard = () => document.querySelector('main.spots-panel') as HTMLElement | null
const box = () => document.querySelector('.pane-frame[data-pane="spots"]') as HTMLElement | null
const callsIn = (root: HTMLElement | null) => [...(root?.querySelectorAll('.sp-row .np-call') ?? [])].map((c) => c.textContent)
const rowIn = (root: () => HTMLElement | null, call: string) => () =>
  ([...(root()?.querySelectorAll('.sp-row') ?? [])] as HTMLElement[]).find((r) => r.querySelector('.np-call')?.textContent === call) ?? null
const neededRow = (call: string) => () =>
  ([...document.querySelectorAll('.np-row:not(.np-header)')] as HTMLElement[]).find((r) => r.querySelector('.np-call')?.textContent?.startsWith(call)) ?? null

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  sessionStorage.clear()
  backend.spots = SPOTS
  backend.needs = []
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: true, phone: true } }))
  localStorage.setItem('nexus.connect.chaseDefault.v1', '1')
  localStorage.setItem(
    'nexus.connect.config',
    JSON.stringify({
      slots: { left1: 'spots', left2: 'bandTiles', right1: 'pota', right2: 'outlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'getout' },
      overlays: {},
    }),
  )
  window.history.replaceState(null, '', '/?panel=spots')
  clearMocks()
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  window.history.replaceState(null, '', '/')
})

/** Mount `panel`'s window, click the row `find` returns once it is there, and return the traffic the click caused. */
async function clickAndRecord(panel: string, find: () => HTMLElement | null): Promise<Call[]> {
  window.history.replaceState(null, '', `/?panel=${panel}`)
  const r = render(<DetachedPanel panel={panel} />)
  await waitFor(() => expect(find(), `the row is on the ${panel} window`).not.toBeNull())
  clearMocks()
  await act(async () => {
    fireEvent.click(find()!)
  })
  await waitFor(() => expect(api.workSpot).toHaveBeenCalled())
  const calls = traffic()
  r.unmount()
  return calls
}

describe('the Spots window', () => {
  it('is the Spots board, with no Pop out of its own', () => {
    render(<DetachedPanel panel="spots" />)
    expect(windowBoard(), 'the Spots board is the window').not.toBeNull()
    expect(windowBoard()!.querySelector('h2')?.textContent).toBe(t('spots.title'))
    expect(document.body.textContent).not.toContain(t('detached.unavailable', { panel: 'spots' }))
    // Already a window of its own: a second Pop out would only focus this one.
    expect(document.querySelector('.np-popout')).toBeNull()
  })

  it("lists the station's spots, asked as it opens and again every 15 s", async () => {
    vi.useFakeTimers()
    render(<DetachedPanel panel="spots" />)
    await act(() => vi.advanceTimersByTimeAsync(100))
    expect(api.getAllSpots, 'asked as the window opens').toHaveBeenCalledTimes(1)
    expect(callsIn(windowBoard())).toEqual(['K1CW', 'JA1FT'])
    await act(() => vi.advanceTimersByTimeAsync(15_000))
    expect(api.getAllSpots, 'asked again 15 s later, the main window’s cadence').toHaveBeenCalledTimes(2)
  })

  for (const [call, expected] of [
    ['K1CW', ['cw', 14.025, '20m', 'K1CW', undefined]],
    ['JA1FT', ['digital', 18.104, '17m', 'JA1FT', 'FT4']],
  ] as const) {
    it(`${call}: a row works through this window's Needed work — the box's traffic and the Needed window's, no transmit command`, async () => {
      const fromWindow = await clickAndRecord('spots', rowIn(windowBoard, call))
      expect(writes(fromWindow)).toEqual([
        ['selectPeer', [call]],
        ['workSpot', [...expected]],
      ])
      const fromBox = await clickAndRecord('connect', rowIn(box, call))
      expect(writes(fromWindow), 'the window worked it exactly as the dashboard window’s Spots box does').toEqual(writes(fromBox))
      // The Needed window, handed the same station as a need — what the Spots view is to App.
      backend.needs = [{ ...spotNeed(SPOTS.find((s) => s.call === call)!), tags: ['NewEntity'] }]
      const fromNeeded = await clickAndRecord('needed', neededRow(call))
      expect(writes(fromWindow), 'the window worked it exactly as the Needed window does').toEqual(writes(fromNeeded))
      for (const verb of TX_VERBS) expect(fromWindow.some(([n]) => n === verb), `the window called ${verb}`).toBe(false)
    })
  }
})

describe("the Spots window and the dashboard window's Spots box are one board", () => {
  it("Hide worked keeps a station by the band-scoped board needs, in both: mode-feature neutral, band scopes honoured", async () => {
    // The CW cockpit switched off. W7B's new entity on 20 m CW still rescues it (the board's needs are
    // mode-feature neutral); W6A's new grid on 20 m does not (New grid is VHF+ unless Settings says otherwise).
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: false, phone: true } }))
    backend.spots = [...SPOTS, ...WORKED]
    backend.needs = [need('W7B', ['NewEntity'], 14.03), need('W6A', ['NewGrid'], 14.035)]
    for (const [panel, root] of [['spots', windowBoard], ['connect', box]] as const) {
      window.history.replaceState(null, '', `/?panel=${panel}`)
      const r = render(<DetachedPanel panel={panel} />)
      await waitFor(() => expect(callsIn(root()), `${panel}: the needed station is rescued`).toContain('W7B'))
      expect(callsIn(root()), `${panel}: a need the band scope leaves off the board rescues nothing`).not.toContain('W6A')
      r.unmount()
    }
  })

  it('both offer Spot me', async () => {
    for (const [panel, root] of [['spots', windowBoard], ['connect', box]] as const) {
      window.history.replaceState(null, '', `/?panel=${panel}`)
      const r = render(<DetachedPanel panel={panel} />)
      await waitFor(() => expect(callsIn(root())).toContain('K1CW'))
      expect(root()!.querySelector('.np-spot-me .np-spot-btn'), `${panel}: Spot me`).not.toBeNull()
      r.unmount()
    }
  })

  it('Spot me posts one spot per press, and never on its own', async () => {
    vi.useFakeTimers()
    render(<DetachedPanel panel="spots" />)
    // Two spot polls, two of the needs and Settings, three of propagation.
    await act(() => vi.advanceTimersByTimeAsync(31_000))
    const press = windowBoard()?.querySelector('.np-spot-me .np-spot-btn') as HTMLButtonElement | null
    expect(press, 'Spot me is in the window').not.toBeNull()
    expect(api.clusterSelfSpot, 'nothing posted before a press').not.toHaveBeenCalled()
    fireEvent.click(press!)
    await act(() => vi.advanceTimersByTimeAsync(31_000))
    expect(api.clusterSelfSpot, 'one press, one spot').toHaveBeenCalledTimes(1)
  })
})
