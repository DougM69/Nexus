// @vitest-environment jsdom
//
// AN ISS PASS NEVER RETUNES A RADIO ANOTHER SCREEN IS USING (2026-10-13). The report: "I will be
// working ft8, I will get a pop up that a satellite is passing. My 20m band changes from 20m to 20m
// custom ... He is not clicking on it, it's doing this on its own." The one path that both pops up
// and retunes with nobody clicking is the ISS SSTV auto-arm, a Settings switch that is remembered:
// at every ISS pass the app-wide propagation poll sent `tune_channel` 145.800 FM to the radio in
// use. On an FT-710 working FT8 on 20 m the radio refused 2 m, the dial read back 14.074 with the
// app's mode left at FM, so the band picker said "20m (custom)", and the band change had already
// turned transmit off.
//
// AT THE WIRE, IN THE REAL APP (stop-control-wiring.test.tsx's idiom): the real `./api` under a
// recording bridge. The scene is the stored opt-in, an ISS pass in progress and the radio on 20 m.
//   · On the FT8 screen the pass is announced and nothing that moves the radio leaves the UI.
//   · On the SSTV screen the auto-arm tunes 145.800 FM exactly as before (the control).
//
// Only `setInterval` is faked, as App.ticker.test.tsx does: the 30 s propagation poll is the clock
// the auto-arm runs on, and the first one fires before the settings file has been read.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import type { AppSnapshot } from './types'
import App from './App'
import { t } from './i18n'
import { resetIssAutoArm } from './features/issAutoArm'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { pastTheSwitch } from './components/ConnectView.testkit'

vi.setConfig({ testTimeout: 60_000, hookTimeout: 30_000 })

type BridgeCall = { cmd: string; args?: Record<string, unknown> }
const bridgeCalls: BridgeCall[] = []

/** Every verb the ISS auto-arm can send: the two tunes, and the SSTV receiver's arm and stop. */
const MOVES_THE_RADIO = new Set(['tune_channel', 'set_frequency', 'sstv_arm', 'sstv_auto_disarm'])

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
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
  harqRescues: 0,
} as unknown as AppSnapshot

/** Enough of a propagation snapshot for the poll to reach the pass tick. */
const propagation = {
  advisory: { headline: '', bands: [], banners: [] },
  openings: [],
  dxpeditions: { workableNow: [], upcoming: [] },
  spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B3.1-class', flare: false, solarWind: null },
  source: 'live',
  asOf: 0,
}

/** The ISS rose a minute ago and has nine minutes to run. */
function issPass() {
  const now = Math.floor(Date.now() / 1000)
  return { name: 'ISS', aosUnix: now - 60, losUnix: now + 540, maxElDeg: 45, aosAzDeg: 200, losAzDeg: 40 }
}

function respond(cmd: string): unknown {
  if (/^(get_snapshot|set_tx_enabled|set_operating_mode|set_area|tune_channel|set_frequency)$/.test(cmd)) return snapshot
  if (cmd === 'get_settings') return { ...defaultSettings, issSstvAutoArm: true }
  if (cmd === 'get_propagation') return propagation
  if (cmd === 'get_iss_pass') return issPass()
  if (/^(get_sstv_state|sstv_)/.test(cmd)) return { sending: false, rxLine: 0, txLine: 0 }
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log)$/.test(cmd)) return []
  if (/^(get_fd_ruleset|get_feed_health|get_xray_now|sat_track_status|get_tle_status|get_kp_forecast|check_for_update)$/.test(cmd)) return null
  if (cmd === 'ask_log') throw new Error('no log in this test')
  return {}
}

/** Let every answer in flight land and React commit what it caused. */
async function settle(): Promise<void> {
  await act(async () => {
    for (let i = 0; i < 20; i++) await new Promise((r) => setTimeout(r, 0))
  })
}

async function advance(ms: number): Promise<void> {
  await act(async () => {
    vi.advanceTimersByTime(ms)
  })
  await settle()
}

/** Mount the real App on `view`, past the first snapshot and the settings file. */
async function mountOn(view: string): Promise<void> {
  window.location.hash = `#${view}`
  render(<App />)
  await settle()
  await advance(300)
  expect(document.querySelector('.app.loading'), 'the boot snapshot landed').toBeNull()
}

/** The next propagation poll, and every command it sent. */
async function nextPoll(): Promise<BridgeCall[]> {
  const from = bridgeCalls.length
  await advance(30_000)
  return bridgeCalls.slice(from)
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  resetIssAutoArm()
  bridgeCalls.length = 0
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      bridgeCalls.push({ cmd, args })
      return respond(cmd) as T
    },
  }
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: { operate: true, sstv: true, sats: true } }),
  )
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  delete window.__TAURI_INTERNALS__
})

describe('the ISS SSTV auto-arm and the screen in use', () => {
  it('on the FT8 screen announces the pass and sends nothing that moves the radio', async () => {
    await mountOn('operate')
    const sent = await nextPoll()
    // The stored opt-in reached the pass tick: without this the silence below proves nothing.
    expect(sent.map((c) => c.cmd)).toContain('get_iss_pass')
    expect(sent.filter((c) => MOVES_THE_RADIO.has(c.cmd))).toEqual([])
    // NOTIFY, NEVER ACT: the pop up still comes.
    expect(document.body.textContent).toContain(t('sat.iss.overhead', { dial: '145.800', mode: 'FM' }))
    expect(document.body.textContent).not.toContain(t('sat.iss.armed', { dial: '145.800', mode: 'FM' }))
  })

  it('on the SSTV screen still tunes 145.800 FM and arms the receiver (control)', async () => {
    await mountOn('sstv')
    const sent = await nextPoll()
    expect(sent.filter((c) => c.cmd === 'tune_channel').map((c) => c.args)).toEqual([
      { dialMhz: 145.8, band: '2m', mode: 'FM' },
    ])
    expect(sent.some((c) => c.cmd === 'sstv_arm' && c.args?.on === true)).toBe(true)
    expect(document.body.textContent).toContain(t('sat.iss.armed', { dial: '145.800', mode: 'FM' }))
  })
})
