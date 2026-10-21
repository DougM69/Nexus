// @vitest-environment jsdom
//
// ⛔ A SPOT WORKED FROM AFAR OPENS ITS COCKPIT AT THE SHACK WITHOUT ARMING TRANSMIT.
//
// A computer driving this station (Operate from this computer) works a spot through `work_spot` with
// `afar`, and the engine enters that section WITHOUT arming Enable TX: from afar only the wired starts
// arm it, under the stream's transmit rules. The Work still moves the hint every window follows
// (`workTick`, `workView`), and this window, open at the shack, followed it into the cockpit and its
// view effect re-asserted the section: `setOperatingMode(mode, false)`, this screen's own
// `set_operating_mode`, which arms the latch for Phone, CW, RTTY and PSK wherever no Stop holds it.
// The authority the Work withheld came back through the shack's own screen. The hint now says where
// the Work came from (`workAfar`), and this window follows a Work from afar into its cockpit without
// the re-assert. The engine's half (the hint, and what that re-assert arms) is
// `crates/tempo-app/src/engine/work_from_afar_tests.rs`; the two meet at `setOperatingMode`'s arguments.
//
// THIS MOUNTS THE REAL APP and stubs only the backend (api) and the waterfall canvas. Recorded: every
// `setOperatingMode` this window sends, with its arguments, and the screen it is left on.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot, NeedAlert } from './types'

const fx = vi.hoisted(() => ({
  rows: [
    {
      call: 'K1CW', entity: 'United States', band: '20m', zone: 5, tags: ['NewBand'], priority: 30,
      headline: 'New band', mode: 'CW', exactMode: null, freqMhz: 14.025, park: null,
    },
  ],
  // The engine's current snapshot (null = the testkit's at boot): every command answers with it, as the
  // engine does, so its work tick only moves on.
  engine: { snap: null as Record<string, unknown> | null },
  // What the engine's Work hint says of the Work behind it: `true` for a Work from afar.
  afar: { value: false },
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers, APP_SNAPSHOT } = await import('./appCockpits.testkit')
  const current = () => (fx.engine.snap ?? APP_SNAPSHOT) as typeof APP_SNAPSHOT & Record<string, unknown>
  return {
    ...auto,
    ...appApiAnswers(),
    getSnapshot: vi.fn(async () => current()),
    setOperatingMode: vi.fn(async () => current()),
    setArea: vi.fn(async () => current()),
    selectPeer: vi.fn(async () => current()),
    getNeedAlerts: vi.fn(async () => fx.rows as unknown as NeedAlert[]),
    // As `work_spot` answers: the tick moves on and the section comes back as the hint, with the
    // Work's origin on a Work from afar (the computer driving the station gets the same answer).
    workSpot: vi.fn(async (mode: string, freqMhz: number, band: string, call?: string) => {
      const s = current()
      const next: Record<string, unknown> = {
        ...s,
        radio: { ...s.radio, dialMhz: freqMhz, band },
        workTick: ((s.workTick as number | undefined) ?? 0) + 1,
        workView: mode,
        workCall: call ?? null,
      }
      if (fx.afar.value) next.workAfar = true
      else delete next.workAfar
      fx.engine.snap = next
      return fx.engine.snap
    }),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    qrzLookup: vi.fn(async () => null),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import App from './App'
import { setOperatingMode, subscribeSnapshot, workSpot } from './api'
import { APP_SNAPSHOT, COCKPIT_MAIN } from './appCockpits.testkit'

// App.neededPark.test.tsx's budget, for the same mount.
vi.setConfig({ testTimeout: 15_000 })

const RAIL = { cw: /^CW operating/, phone: /^Phone \(SSB\) operating/ } as const
/** The screen each Work's section opens (`workHintView`). */
const SCREEN: Record<string, string> = { cw: 'cw', phone: 'phone', rtty: 'rtty', keyboard: 'psk' }

let push: ((s: AppSnapshot) => void) | null = null

beforeEach(() => {
  localStorage.clear()
  // The Needed board is a core section; the auto-pop would move it into its own window.
  localStorage.setItem('nexus.needed.autopop', 'off')
  fx.engine.snap = null
  fx.afar.value = false
  push = null
  vi.mocked(setOperatingMode).mockClear()
  vi.mocked(workSpot).mockClear()
  vi.mocked(subscribeSnapshot).mockImplementation(((fn: (s: AppSnapshot) => void) => {
    push = fn
    return () => {}
  }) as never)
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  vi.mocked(subscribeSnapshot).mockImplementation(() => () => {})
})

/** Every `setOperatingMode` this window has sent, with its arguments. */
const asserted = () => vi.mocked(setOperatingMode).mock.calls.map((c) => [...c])

/** The engine pushes a snapshot; from then on it is the one every command answers with. */
function send(s: Record<string, unknown>) {
  fx.engine.snap = { ...APP_SNAPSHOT, ...s }
  act(() => push!(fx.engine.snap as unknown as AppSnapshot))
}

/** Boot this window on `view`, and hand it the engine's first snapshot: the baseline, never followed. */
async function boot(view: string) {
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(push).not.toBeNull())
  send({ workTick: 0 })
  vi.mocked(setOperatingMode).mockClear()
}

/** Wait for `view`'s cockpit to mount and the window to say it is there, then let its effects land. */
async function landedOn(view: string) {
  await waitFor(() => {
    expect(document.querySelector(COCKPIT_MAIN[view]), `the ${view} cockpit never mounted`).not.toBeNull()
    expect(localStorage.getItem('nexus.view')).toBe(view)
  })
  expect(document.querySelector('.view-crash')).toBeNull()
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50))
  })
}

describe('a spot worked from afar: this window opens its cockpit and asserts nothing', () => {
  it('CW, Phone, RTTY and PSK: each cockpit opens, and no setOperatingMode is sent', async () => {
    await boot('operate')
    let tick = 0
    for (const section of ['cw', 'phone', 'rtty', 'keyboard']) {
      send({ workTick: ++tick, workView: section, workCall: 'K1ABC', workAfar: true })
      await landedOn(SCREEN[section])
    }
    // The red this pins: [['cw', false], ['phone', false], ['rtty', false], ['keyboard', false]], each
    // the shack screen's own `set_operating_mode` with no `afar`, which arms the latch.
    expect(asserted(), 'a Work from afar was re-asserted from the shack’s screen').toEqual([])
    // CONTROL, same window: a Work the station's own screens made re-asserts its section as it always
    // has, so the recorder above was live and the hint was read.
    send({ workTick: ++tick, workView: 'cw', workCall: 'K1ABC' })
    await landedOn('cw')
    expect(asserted()).toEqual([['cw', false, false]])
  })

  it('the operator’s own rail clicks afterwards re-assert as before', async () => {
    await boot('operate')
    send({ workTick: 1, workView: 'cw', workCall: 'K1ABC', workAfar: true })
    await landedOn('cw')
    expect(asserted(), 'a Work from afar was re-asserted from the shack’s screen').toEqual([])
    // The operator at the shack takes Phone, then CW, from the rail: each is entered as it always was.
    fireEvent.click(screen.getByRole('button', { name: RAIL.phone }))
    await landedOn('phone')
    fireEvent.click(screen.getByRole('button', { name: RAIL.cw }))
    await landedOn('cw')
    expect(asserted()).toEqual([
      ['phone', true, false],
      ['cw', true, false],
    ])
  })

  // A Work from afar into the screen this window is already on changes no screen, so nothing is
  // entered; and it leaves nothing behind that could hold back the operator's own next entry there.
  it('into the screen already open: nothing sent, and the operator’s own return there re-asserts', async () => {
    await boot('cw')
    send({ workTick: 1, workView: 'cw', workCall: 'K1ABC', workAfar: true })
    await landedOn('cw')
    expect(asserted()).toEqual([])
    fireEvent.click(screen.getByRole('button', { name: RAIL.phone }))
    await landedOn('phone')
    fireEvent.click(screen.getByRole('button', { name: RAIL.cw }))
    await landedOn('cw')
    expect(asserted()).toEqual([
      ['phone', true, false],
      ['cw', true, false],
    ])
  })
})

describe('controls: a Work made at the station re-asserts exactly as before', () => {
  it('a hint from the station’s own pop-out: the cockpit opens and its section is re-asserted', async () => {
    await boot('operate')
    let tick = 0
    for (const section of ['cw', 'phone', 'rtty', 'keyboard']) {
      send({ workTick: ++tick, workView: section, workCall: 'K1ABC' })
      await landedOn(SCREEN[section])
    }
    expect(asserted()).toEqual([
      ['cw', false, false],
      ['phone', false, false],
      ['rtty', false, false],
      ['keyboard', false, false],
    ])
  })

  it('this window’s own Needed click: the Work, then the CW cockpit re-asserted once', async () => {
    await boot('needed')
    const row = await waitFor(() => {
      const r = screen.getAllByRole('row').find((el) => el.getAttribute('aria-label')?.includes('K1CW'))
      expect(r).toBeTruthy()
      return r as HTMLElement
    })
    fireEvent.click(row)
    await landedOn('cw')
    expect(vi.mocked(workSpot).mock.calls).toEqual([['cw', 14.025, '20m', 'K1CW', undefined]])
    expect(asserted()).toEqual([['cw', false, false]])
  })
})
