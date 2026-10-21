// @vitest-environment jsdom
//
// THE PHONE TAB ON THE SSTV SCREEN KEEPS THE DIAL (2026-10-20). The operator: "when on sstv and
// changing to phone, it should not switch to phone frequency until I tell it to". The SSTV screen
// claims no section on entry (RX-first), so the station is often still in FT8's when the operator
// gets there, and the Phone tab then followed to the phone segment default.
//
// AT THE WIRE, IN THE REAL App (stop-control-wiring.test.tsx's idiom): the real `./api` under a
// recording bridge, so every assertion is about the argument object that left the UI.
//   · Phone from the SSTV screen follows AND asks the engine to keep the dial (`keepDial`); the
//     engine keeps it where Phone may transmit and homes it where it may not.
//   · With Phone already the section homed for, Phone from the SSTV screen re-asserts without
//     following and still asks (`keepDial`): the dial stays either way, and the ask carries the FM
//     channel the SSTV screen holds into Phone (the rule: a kept FM channel keeps
//     commanding FM in Phone).
//   · Every other entry sends exactly what it sent before: Phone from FT8, followed or not, and CW
//     from SSTV.
//   · Entering the SSTV screen still asserts nothing (RX-first).
//   · The backend has a `keep_dial` parameter to receive it.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { AppSnapshot } from './types'
import App from './App'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { pastTheSwitch } from './components/ConnectView.testkit'

type BridgeCall = { cmd: string; args?: Record<string, unknown> }
const bridgeCalls: BridgeCall[] = []

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.23,
    band: '20m',
    catOk: true,
    sideband: 'USB',
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
  harqRescues: 0,
} as unknown as AppSnapshot

/** The section the settings file says the operator last worked in: it seeds the home guard. */
let savedSection = 'digital'

function respond(cmd: string): unknown {
  if (/^(get_snapshot|set_operating_mode|set_area|set_tx_enabled)$/.test(cmd)) return snapshot
  if (cmd === 'get_settings') return { ...defaultSettings, operatingMode: savedSection }
  if (/^(get_sstv_state|sstv_)/.test(cmd)) return { sending: false, rxLine: 0, txLine: 0 }
  if (/^(cw_decode|get_cw_state)$/.test(cmd)) {
    return { text: '', wpm: 20, sent: [], keyerError: null, candidates: [], rst: null, name: null, state: 'listening',
      headline: '', prompt: '', recommended: null, workedCall: null }
  }
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|get_xray_now|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update)$/.test(cmd)) return null
  if (cmd === 'ask_log') throw new Error('no log in this test')
  if (cmd === 'client_status') return null
  return {}
}

/** Let every answer in flight land and React commit what it caused. */
async function settle(): Promise<void> {
  await act(async () => {
    for (let i = 0; i < 20; i++) await new Promise((r) => setTimeout(r, 0))
  })
}

/** Mount the real App on `view`, past the boot snapshot and the settings file (whose saved section
 *  seeds the home guard). */
async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 10_000 })
  await waitFor(() => expect(bridgeCalls.some((c) => c.cmd === 'get_settings')).toBe(true))
  await settle()
}

/** The navigation's button for a section, by its visible label, and what the click sent to
 *  `set_operating_mode`. */
async function entriesSentBy(label: string): Promise<Record<string, unknown>[]> {
  const btn = [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
  expect(btn, `no navigation button labelled ${label}`).toBeTruthy()
  const from = bridgeCalls.length
  fireEvent.click(btn!)
  await settle()
  return bridgeCalls
    .slice(from)
    .filter((c) => c.cmd === 'set_operating_mode')
    .map((c) => c.args ?? {})
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  bridgeCalls.length = 0
  savedSection = 'digital'
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      bridgeCalls.push({ cmd, args })
      return respond(cmd) as T
    },
  }
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: { operate: true, phone: true, cw: true, sstv: true } }),
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
})
afterEach(() => {
  cleanup()
  delete window.__TAURI_INTERNALS__
})

describe('the Phone tab on the SSTV screen', () => {
  it('follows and asks the engine to keep the dial', async () => {
    await mountOn('sstv')
    expect(await entriesSentBy('Phone')).toEqual([{ mode: 'phone', followFreq: true, keepDial: true }])
  })

  it('CONTROL: from the FT8 screen the Phone tab follows as before, with no keepDial', async () => {
    await mountOn('operate')
    expect(await entriesSentBy('Phone')).toEqual([{ mode: 'phone', followFreq: true }])
  })

  it('CONTROL: from the SSTV screen the CW tab follows as before, with no keepDial', async () => {
    await mountOn('sstv')
    expect(await entriesSentBy('CW')).toEqual([{ mode: 'cw', followFreq: true }])
  })

  it('with Phone the section already homed for, re-asserts without following and still asks to keep the dial', async () => {
    savedSection = 'phone'
    await mountOn('sstv')
    expect(await entriesSentBy('Phone')).toEqual([{ mode: 'phone', followFreq: false, keepDial: true }])
  })

  it('CONTROL: with Phone the section already homed for, from the FT8 screen it re-asserts as before', async () => {
    savedSection = 'phone'
    await mountOn('operate')
    expect(await entriesSentBy('Phone')).toEqual([{ mode: 'phone', followFreq: false }])
  })

  it('CONTROL: entering the SSTV screen still asserts nothing', async () => {
    await mountOn('operate')
    expect(await entriesSentBy('SSTV')).toEqual([])
  })

  it('reaches a `keep_dial` parameter on the backend command', () => {
    const lib = readFileSync(resolve(__dirname, '../../src-tauri/src/lib.rs'), 'utf8')
    const m = /\n(?:pub )?(?:async )?fn set_operating_mode\(([^)]*)\)/s.exec(lib)
    expect(m, 'no `fn set_operating_mode(` in src-tauri/src/lib.rs').not.toBeNull()
    for (const p of ['mode', 'follow_freq', 'keep_dial']) {
      expect(new RegExp(`\\b${p}\\s*:`).test(m![1]), `set_operating_mode has no \`${p}:\` parameter`).toBe(true)
    }
  })
})
