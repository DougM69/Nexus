// @vitest-environment jsdom
// The hosted page's JS8 view, mounted as the page mounts it (the whole Remote application over the
// application transport, this browser observing), against what each station sends. A 1.19.0 station
// sends the desktop's own JS8 keys, and the page shows what the station's JS8 screen shows from
// them: HB NET and what holds HB ACK back, the call list's ★ and ☎, the Inbox's note on a held
// message too long for this speed, the waterfall's filter plate, and the RX pane's one line per
// message. None of them writes from here: the station's Remote vocabulary has no command for any.
// A 1.18.0 station sends none of them, and the same page shows its JS8 view as it always has.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import App from '../App'
import { installApplicationTransport } from '../applicationTransport'
import type { AppSnapshot, Settings } from '../types'
import settingsFixture from '../components/__fixtures__/defaultSettings.json'
import { StationControlContext, StationDataContext } from '../stationAccess'
import js8Fixture from './__fixtures__/js8.json'
import js8v118 from './__fixtures__/js8-1.18.0.json'
import { parseJs8Sample } from './js8'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import type { ApplicationClient } from './application-client'
import type { QueryPage } from './application-query-protocol'
import { pastTheSwitch } from '../components/ConnectView.testkit'
import { t } from '../i18n'
import { JS8_SPEEDS } from '../js8Vocab'

// The whole application mounts per test, as in BrowserApplication.test.tsx (its note on why).
vi.setConfig({ testTimeout: 30_000 })

const snapshot = {
  mycall: 'N0CALL', mygrid: 'AA00', mode: 'Normal',
  radio: { dialMhz: 7.078, band: '40m', catOk: true, sideband: 'USB', operatingMode: 'digital',
    transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500,
    txLevel: 0.5, slot: 0, nextSlotMs: 12000 },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null, recentDecodes: [], harqRescues: 0,
} as unknown as AppSnapshot
/** The station's settings, `station` over the defaults, as its Remote projection sends them (SETTINGS_KEYS,
 *  read from the Rust): a setting the projection leaves out never reaches the page. */
function projectedSettings(station: Partial<Settings> = {}): Settings {
  const rust = readFileSync(resolve('../src-tauri/src/remote_service/application.rs'), 'utf8')
  const keys = [...rust.match(/const SETTINGS_KEYS: &\[&str\] = &\[([\s\S]*?)\];/)![1].matchAll(/"([a-zA-Z0-9]+)"/g)].map((m) => m[1])
  expect(keys).toContain('js8CallsignAgingMin')
  return Object.fromEntries(Object.entries({ ...settingsFixture, ...station }).filter(([key]) => keys.includes(key))) as unknown as Settings
}
/** The reads the page makes with JS8 open; anything else this browser sends is a write. */
const READS = ['get_snapshot', 'get_settings', 'get_band_plan', 'get_js8_state', 'get_spectrum_row', 'get_meters',
  'app_version', 'dxcc_entity_locations', 'get_declination', 'get_awards', 'get_journey', 'radio_launch_info',
  'check_for_update', 'get_propagation', 'sat_track_status', 'get_tle_status', 'get_kp_forecast', 'get_xray_now',
  'get_dxped_windows', 'get_feed_health', 'get_need_alerts', 'get_all_spots', 'get_fd_ruleset', 'log_operators', 'read_rotator', 'get_sat_transponder']
/** What the controls these keys bring would call at the station. */
const WRITES = ['js8_set_filter', 'js8_set_qsy_center', 'js8_qsy_to_centre', 'js8_hop_answer', 'js8_arm', 'js8_heartbeat_now',
  'js8_set_rx_blocklist', 'js8_set_groups', 'js8_set_avoid_allcall']

let dispose: (() => void) | undefined
beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  window.matchMedia = ((media: string) => ({ matches: false, media, addEventListener() {}, removeEventListener() {},
    addListener() {}, removeListener() {} })) as unknown as typeof window.matchMedia
})
afterEach(() => { cleanup(); dispose?.(); vi.unstubAllGlobals(); vi.restoreAllMocks() })

/** Mount the page against a station that sends `sample` and has `stationSettings` among its settings,
 *  open JS8, and wait for its first poll. */
async function openJs8(sample: { state: object; capturedAtMs: number }, stationSettings: Partial<Settings> = {}) {
  const calls: string[] = []
  dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(command: string): Promise<T> => {
    calls.push(command)
    if (command === 'get_snapshot') return structuredClone(snapshot) as T
    if (command === 'get_settings') return projectedSettings(stationSettings) as T
    if (command === 'get_band_plan') return [] as T
    if (command === 'get_js8_state') return parseJs8Sample(structuredClone(sample), 50) as T
    if (command === 'get_spectrum_row') return { row: [], loHz: 0, hiHz: 4000, source: 'audio' } as T
    if (command === 'get_meters') return { rxLevel: 0, smeterDb: null, cwToneHz: null } as T
    throw new Error('applicationUnsupported')
  } })
  const context = new RemoteCollections({ supports: () => false } as unknown as ApplicationClient)
  vi.spyOn(context, 'page').mockImplementation(async () => ({ collection: 'js8Context', offset: 0, total: 0, retained: 0, rows: [], nextCursor: null, ageMs: 0,
    meta: { capturedAgeMs: 0, source: { plan: [], history: { W1AW: { count: 2, lastUnix: 1700000000, grid: 'FN31', name: 'PRIOR CONTACT', comment: 'COMPLETE LOG' },
      K2ABC: { count: 0, lastUnix: null, grid: '', name: '', comment: '' } } } } } as unknown as QueryPage))
  const { container } = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteCollectionsContext.Provider value={context}>
      <App remote={{ snapshot: structuredClone(snapshot), settings: projectedSettings(stationSettings), bandPlan: [], js8: true, status: <div>Observer</div> }} />
    </RemoteCollectionsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  fireEvent.click(screen.getByRole('button', { name: /^JS8 —/ }))
  await waitFor(() => expect(container.querySelector('.js8-stations')?.textContent).toContain('COMPLETE LOG'))
  const root = container.querySelector<HTMLElement>('.js8-cockpit')!
  const station = (call: string) =>
    [...root.querySelectorAll('.js8-station:not(.js8-group)')].find((r) => r.querySelector('.js8-station-call')?.textContent === call)!
  const mark = (call: string) => [...station(call).querySelectorAll('.js8-mark .js8-chip')].map((e) => e.className)
  const rxLines = () => [...root.querySelectorAll('.js8-rx .js8-rx-line .js8-text')].map((e) => e.textContent)
  return { root, calls, station, mark, rxLines }
}

it('shows a 1.19.0 station’s HB NET, ★ and ☎, too-long note, filter plate and RX lines, and writes none of them', async () => {
  const { root, calls, station, mark, rxLines } = await openJs8(js8Fixture)
  // HB NET as the station has it, read-only; HB shows, as the station's does with it on at Normal.
  const hbNet = root.querySelector<HTMLButtonElement>('.js8-hbnet')!
  expect([hbNet.getAttribute('aria-pressed'), hbNet.disabled, hbNet.title]).toEqual(['true', true, t('js8.dock.hbNet.title.on')])
  expect(root.querySelector('.js8-hb')).not.toBeNull()
  // ⚑ for W1AW (a message for me goes ahead of its ☎), ★ for K2ABC.
  expect(mark('W1AW')).toEqual(['js8-chip js8-mark-message'])
  expect(mark('K2ABC')).toEqual(['js8-chip js8-mark-ack'])
  // The held message says why it is not offered, in the station's numbers.
  const note = js8Fixture.state.inboxTooLong[0]
  expect(root.querySelector('.js8-inbox-row.state-store .js8-path')?.textContent).toBe(
    t('js8.inbox.tooLong.fits', { frames: note.frames, max: note.max, speed: JS8_SPEEDS[note.fitsAt as 'fast'].label }))
  // The RX pane: one line a message, closed by the end mark.
  expect(rxLines()).toEqual(js8Fixture.state.activity.map((r) => `${r.text} ♢ `))
  // The filter plate names the station's filter and opens its controls, every one of them shut here.
  const plate = root.querySelector<HTMLButtonElement>('.js8-filter-pill')!
  expect(plate.textContent).toBe(t('js8.wf.pill.on', { lo: 500, hi: 2500 }))
  fireEvent.click(plate)
  const dialog = await waitFor(() => { const d = document.querySelector('.js8-wf-dialog'); expect(d).not.toBeNull(); return d! })
  const shut = [...dialog.querySelectorAll<HTMLInputElement | HTMLButtonElement>('input, button')]
  expect(shut.length).toBe(6)
  for (const control of shut) {
    expect(control.disabled, control.outerHTML).toBe(true)
    fireEvent.click(control)
  }
  fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' })
  fireEvent.click(hbNet)
  // A row's menu offers "Centre filter" while the station's filter is on, not to this browser.
  await act(async () => { fireEvent.pointerDown(station('K2ABC').querySelector('.js8-row-menu')!, { button: 0, pointerType: 'mouse' }) })
  const centre = await waitFor(() => {
    const item = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((e) => e.textContent === t('js8.wf.centreAt', { hz: 1500 }))
    expect(item).toBeTruthy()
    return item!
  })
  expect(centre.getAttribute('aria-disabled') === 'true' || centre.hasAttribute('data-disabled')).toBe(true)
  await act(async () => { fireEvent.click(centre) })
  expect(calls.filter((c) => !READS.includes(c))).toEqual([])
  for (const write of WRITES) expect(calls).not.toContain(write)
})

it('says what holds HB ACK back and which stations the 55-minute rule stopped, and raises the band schedule’s notice without its Cancel', async () => {
  const value = structuredClone(js8Fixture)
  const hop = { mhz: 7.078, firesAtMs: Date.now() + 25_000, description: '40 m evening net', held: false }
  Object.assign(value.state, { txEnabled: true, autoreply: false, relay: false, hbAck: true, hbOn: false,
    armed: { autoreply: false, relay: false, hbAck: true, hb: false, cq: false },
    notArmed: { autoreply: null, relay: null, hbAck: null, hb: null, cq: 'unscheduled' } })
  Object.assign(value.state.presence, { hbPaused: true, hbAckSuppressed: 'qsoPause', hbAckRateBlocked: ['K9XYZ', 'W9ABC'] })
  ;(value.state.controls as { hop: unknown }).hop = hop
  const { root, calls } = await openJs8(value)
  const hbAck = root.querySelector<HTMLButtonElement>('.js8-hback')!
  expect([hbAck.getAttribute('aria-pressed'), hbAck.classList.contains('armed'), hbAck.disabled]).toEqual(['true', false, true])
  expect(hbAck.title).toContain(t('js8.dock.hbAck.suppressed.qsoPause'))
  expect(hbAck.title).toContain(t('js8.dock.hbAck.rateBlocked', { calls: 'K9XYZ, W9ABC' }))
  expect(root.querySelector('.js8-hb')?.getAttribute('title')).toBe(t('js8.dock.hb.paused'))
  const toast = await waitFor(() => {
    const shown = [...document.querySelectorAll('.ui-toast')].find((e) => e.textContent?.includes(hop.description))
    expect(shown).toBeTruthy()
    return shown!
  })
  expect(toast.querySelector('.ui-toast-action')).toBeNull()
  expect(calls.filter((c) => !READS.includes(c))).toEqual([])
})

it('shows a 1.18.0 station’s JS8 view as the page always has', async () => {
  const { root, mark, rxLines } = await openJs8(js8v118)
  expect(root.querySelector('.js8-hbnet')).toBeNull()
  expect(root.querySelector('.js8-hb')).not.toBeNull()
  expect(root.querySelector('.js8-filter-pill')).toBeNull()
  expect(root.querySelector('.js8-inbox-row .js8-path')).toBeNull()
  expect(mark('W1AW')).toEqual(['js8-chip js8-mark-message'])
  expect(mark('K2ABC')).toEqual([])
  // A feed that does not say where its rows stand: each row on its own line, as it stands.
  expect(rxLines()).toEqual(js8v118.state.activity.map((r) => r.text))
  expect(root.querySelector('.js8-observation-status')?.textContent).toBe(t('remote.applicationObserver'))
})

it('follows the station’s JS8 groups, RX block list and @ALLCALL choice, none of which it can change', async () => {
  const value = structuredClone(js8Fixture)
  const row = value.state.activity[0]
  value.state.activity.push(
    // A message to a group the station joined, away from my offset, and a station's call to me.
    { ...row, from: 'W1AW', text: 'W1AW: @FUN GROUP NEWS', freqHz: 900 },
    { ...row, from: 'K9BLK', text: 'K9BLK: N0CALL HELLO', freqHz: 1100 })
  value.state.stations.push({ call: 'K9BLK', grid: null, snrDb: -12, freqHz: 1100, speed: 'normal', lastMs: 1700000000000,
    lastHb: false, lastCq: false, storedMsgs: 0 } as (typeof value.state.stations)[0])
  value.state.presence.avoidAllcall = true
  const { root, calls, station, mark, rxLines } = await openJs8(value,
    { js8Groups: ['@FUN'], js8RxBlocklist: ['K9BLK'], js8AvoidAllcall: true } as Partial<Settings>)
  // The call list: the joined group on top, and no @ALLCALL, which the station has left.
  expect([...root.querySelectorAll('.js8-station.js8-group .js8-station-call')].map((e) => e.textContent)).toEqual(['@FUN'])
  // The RX pane: the group's message, and nothing from the blocked station, which the list marks ✕.
  expect(rxLines()).toContain('W1AW: @FUN GROUP NEWS \u2662 ')
  expect(rxLines().filter((l) => l?.includes('K9BLK'))).toEqual([])
  expect(mark('K9BLK')).toEqual(['js8-chip js8-mark-blocked'])
  // Its row menu offers Unblock, greyed: the list is changed at the station only, and so is the group.
  await act(async () => { fireEvent.pointerDown(station('K9BLK').querySelector('.js8-row-menu')!, { button: 0, pointerType: 'mouse' }) })
  const unblock = await waitFor(() => {
    const item = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((e) => e.textContent === t('js8.lists.unblock'))
    expect(item).toBeTruthy()
    return item!
  })
  expect(unblock.getAttribute('aria-disabled') === 'true' || unblock.hasAttribute('data-disabled')).toBe(true)
  await act(async () => { fireEvent.click(unblock) })
  expect(calls.filter((c) => !READS.includes(c))).toEqual([])
  for (const write of WRITES) expect(calls).not.toContain(write)
})
