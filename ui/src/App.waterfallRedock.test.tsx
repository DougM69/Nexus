// @vitest-environment jsdom
//
// THE TORN-OFF WATERFALL COMES BACK WHEN ITS WINDOW CLOSES, HOWEVER IT CLOSES.
//
// While the waterfall is out in a window of its own, the Operate cockpit shows a re-dock bar where it was, and
// the window closing puts the waterfall back. The window used to say it was going only by clearing a flag in
// `beforeunload`, which the cockpit hears as a `storage` event. On Windows the webview is closed by WebView2's
// `Close`, which "is synchronous and won't trigger the beforeunload event", so a window closed from its title
// bar or with Alt+F4 left the bar standing and the flag set. The cockpit now also hears Tauri's own
// `tauri://destroyed` for that window, as the main window hears the Contest screen's window
// (App.contestScreenPopOut.test.tsx).
//
// AT THE WIRE, IN THE REAL APP: the real `./api` under a recording bridge, the real Operate cockpit with its
// stored panel record, and Tauri's event bridge, which is what says a window is gone.
//   · The window's close brings the waterfall back with its flag still set (no `beforeunload`, no `storage`
//     event), and the record says docked.
//   · Another window's close leaves it out.
//   · It comes back while another screen is showing: the cockpit stays mounted, hidden, behind it.
//   · A waterfall the operator removed stays removed when the window closes.
//   · The re-dock bar still brings it back and closes the window.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, waitFor, fireEvent, act } from '@testing-library/react'
import App from './App'
import { allFeatureIds } from './features/registry'
import { pastTheSwitch } from './components/ConnectView.testkit'
import { __resetDurableForTest } from './features/durableStore'
import { WATERFALL_DETACHED_KEY } from './features/panelState'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000, hookTimeout: 30_000 })

// ── the recorder ────────────────────────────────────────────────────────────────────────────
type BridgeCall = { cmd: string; args?: Record<string, unknown> }
const bridgeCalls: BridgeCall[] = []
const mark = (): number => bridgeCalls.length
const sentSince = (from: number, cmd: string) => bridgeCalls.slice(from).filter((c) => c.cmd === cmd).map((c) => c.args)

// ── Tauri's event bridge ────────────────────────────────────────────────────────────────────
// `window.__TAURI__.event.listen(event, handler, { target })`, as `withGlobalTauri` exposes it. When a window is
// destroyed, Tauri sends `tauri://destroyed` to every listener whose target names that window, and to every
// listener with no target (tauri 2, `manager/window.rs` `on_window_event` → `emit_to_window`, and
// `event/listener.rs` `match_any_or_filter`). A listener with no target hears every window's close, so a test
// that closes a DIFFERENT window tells the two apart.
type Target = { kind: string; label?: string }
type Listener = { event: string; target: Target; handler: (e: { event: string; id: number; payload: unknown }) => void }
const listeners: Listener[] = []
function installEventBridge() {
  ;(window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    event: {
      listen: async (event: string, handler: Listener['handler'], options?: { target?: string | Target }) => {
        const target: Target =
          typeof options?.target === 'string' ? { kind: 'AnyLabel', label: options.target } : (options?.target ?? { kind: 'Any' })
        const l = { event, target, handler }
        listeners.push(l)
        return () => {
          const i = listeners.indexOf(l)
          if (i >= 0) listeners.splice(i, 1)
        }
      },
    },
  }
}
/** The window labelled `label` is destroyed, closed any way at all. */
async function windowDestroyed(label: string) {
  await act(async () => {
    for (const l of [...listeners]) {
      if (l.event !== 'tauri://destroyed') continue
      const named = (l.target.kind === 'Window' || l.target.kind === 'WebviewWindow') && l.target.label === label
      if (l.target.kind === 'Any' || named) l.handler({ event: 'tauri://destroyed', id: 1, payload: null })
    }
  })
}

// ── the station: FT8 on 20 m, nothing on the air ────────────────────────────────────────────
const snapshot = () => ({
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    rigMode: 'USB',
    operatingMode: 'digital',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    tuning: false,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'Ft8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  highlights: [],
  harqRescues: 0,
})

function respond(cmd: string): unknown {
  if (/^(get_snapshot|halt_tx|set_operating_mode|set_area|set_mode|select_peer|set_settings)$/.test(cmd)) return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52' }
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|get_xray_now|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update|resolve_entity|get_call_history|get_spectrum_row)$/.test(cmd)) return null
  if (cmd === 'ask_log') throw new Error('no log in this test')
  // A computer running its own station: the backend answers None, and the banner asks once. The
  // catch-all's `{}` would read as driving a remote station, asked every half second.
  if (cmd === 'client_status') return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  __resetDurableForTest()
  pastTheSwitch()
  bridgeCalls.length = 0
  listeners.length = 0
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      bridgeCalls.push({ cmd, args })
      return respond(cmd) as T
    },
  }
  installEventBridge()
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: Object.fromEntries(allFeatureIds().map((id) => [id, true])) }),
  )
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  delete window.__TAURI_INTERNALS__
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
  window.history.replaceState(null, '', '/')
})

/** The main window's Operate record, as stored: what the waterfall is (docked, popped, removed). */
const OPERATE_RECORD = 'nexus.panels.operate.main'
const storedWaterfall = (): string | undefined =>
  (JSON.parse(localStorage.getItem(OPERATE_RECORD) ?? '{}') as { state?: { waterfall?: string } }).state?.waterfall
/** The Operate cockpit, which App keeps mounted (hidden) whatever screen is showing. */
const operateHost = (): HTMLElement => document.querySelector<HTMLElement>('.operate-host')!
/** The docked waterfall's ⧉, by its title: four buttons in the waterfall's header share its class. */
const dockedWaterfall = () =>
  [...operateHost().querySelectorAll<HTMLButtonElement>('.cockpit-waterfall button')].find((b) => b.title === t('waterfall.popOut.title')) ??
  null
const redockBar = () => operateHost().querySelector<HTMLButtonElement>('.wf-redock')

/** The real App on `view`, past its boot snapshot, with the Operate cockpit mounted. */
async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 15_000 })
  await waitFor(() => expect(document.querySelector('.operate-host .cockpit-body'), 'control: the Operate cockpit is mounted').toBeTruthy(), {
    timeout: 15_000,
  })
}

/** Press the waterfall's ⧉, and stand in for the window it opens, which sets its flag as it starts. */
async function popOut(): Promise<void> {
  const from = mark()
  fireEvent.click(dockedWaterfall()!)
  await waitFor(() => expect(redockBar(), 'the cockpit never showed the re-dock bar').not.toBeNull())
  expect(sentSince(from, 'open_panel_window'), 'the ⧉ opened no waterfall window').toEqual([{ panel: 'waterfall' }])
  expect(dockedWaterfall(), 'control: the docked waterfall went with it').toBeNull()
  expect(storedWaterfall(), 'control: the record says it is out').toBe('popped')
  localStorage.setItem(WATERFALL_DETACHED_KEY, '1')
}

describe('the torn-off waterfall comes back when its window closes', () => {
  it('its window’s close re-docks it with the flag still set: no beforeunload, no storage event, as on Windows', async () => {
    await mountOn('operate')
    expect(dockedWaterfall(), 'control: the waterfall starts docked').not.toBeNull()
    await popOut()
    await windowDestroyed('panel-waterfall')
    await waitFor(() => expect(redockBar(), 'the window closed and the re-dock bar is still up').toBeNull())
    expect(dockedWaterfall(), 'the waterfall is not back in the cockpit').not.toBeNull()
    expect(storedWaterfall(), 'the record still says it is out').toBe('docked')
    expect(localStorage.getItem(WATERFALL_DETACHED_KEY), 'control: nothing cleared the flag').toBe('1')
  })

  it('another window’s close leaves it out', async () => {
    await mountOn('operate')
    await popOut()
    for (const label of ['panel-contestscreen', 'panel-operate', 'panel-needed', 'main']) await windowDestroyed(label)
    expect(redockBar(), 'another window’s close took the re-dock bar away').not.toBeNull()
    expect(dockedWaterfall(), 'another window’s close re-docked the waterfall').toBeNull()
    expect(storedWaterfall()).toBe('popped')
  })

  it('it comes back while another screen is showing', async () => {
    localStorage.setItem(OPERATE_RECORD, JSON.stringify({ v: 1, state: { waterfall: 'popped' }, share: {} }))
    localStorage.setItem(WATERFALL_DETACHED_KEY, '1')
    await mountOn('logbook')
    expect(operateHost().hidden, 'control: the Operate cockpit is behind another screen').toBe(true)
    expect(redockBar(), 'control: the waterfall starts out').not.toBeNull()
    await windowDestroyed('panel-waterfall')
    await waitFor(() => expect(storedWaterfall(), 'the record still says it is out').toBe('docked'))
    expect(redockBar()).toBeNull()
    expect(dockedWaterfall(), 'the waterfall is not back in the cockpit').not.toBeNull()
  })

  it('a waterfall the operator removed stays removed when the window closes', async () => {
    localStorage.setItem(OPERATE_RECORD, JSON.stringify({ v: 1, state: { waterfall: 'removed' }, share: {} }))
    localStorage.setItem(WATERFALL_DETACHED_KEY, '1')
    await mountOn('operate')
    expect(operateHost().querySelector('.cockpit-waterfall'), 'control: the waterfall starts removed').toBeNull()
    await windowDestroyed('panel-waterfall')
    expect(operateHost().querySelector('.cockpit-waterfall'), 'the window’s close brought back a removed waterfall').toBeNull()
    expect(redockBar()).toBeNull()
    expect(storedWaterfall()).toBe('removed')
  })

  it('the re-dock bar still brings it back and closes the window, and that close changes nothing more', async () => {
    await mountOn('operate')
    await popOut()
    const from = mark()
    fireEvent.click(redockBar()!)
    await waitFor(() => expect(dockedWaterfall(), 'the bar did not bring the waterfall back').not.toBeNull())
    expect(sentSince(from, 'close_panel_window'), 'the bar left the window open').toEqual([{ panel: 'waterfall' }])
    expect(storedWaterfall()).toBe('docked')
    // The close the bar asked for arrives: the waterfall is already back, and stays so.
    await windowDestroyed('panel-waterfall')
    expect(dockedWaterfall()).not.toBeNull()
    expect(redockBar()).toBeNull()
    expect(storedWaterfall()).toBe('docked')
  })
})
