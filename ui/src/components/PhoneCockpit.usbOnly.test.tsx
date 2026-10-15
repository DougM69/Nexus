// @vitest-environment jsdom
//
// 60 M ALLOWS ONLY USB PHONE, AND THE COCKPIT SAYS SO BEFORE THE LOCK DOES.
//
// Every US class may talk on 60 m in upper sideband only; the engine refuses LSB, AM and FM there
// and tells the cockpit two things (`Engine::phone_mode_refused`, `privileges::usb_only_phone`):
//   · `phoneUsbOnly` — the rule holds where this radio transmits, so the mode picker greys LSB,
//     AM and FM with the rule as their tooltip, instead of offering them and then showing
//     TX LOCKED;
//   · `phoneModeRefused` — the lock is the MODE alone (USB would key), so the PTT's tooltip and
//     the toast a press raises name the rule, not "this frequency is outside your license
//     privileges". A frequency USB is refused on too keeps that sentence.
// This suite asserts what the operator is TOLD and what a greyed button sends. The engine's gate
// is untouched by it.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act } from '@testing-library/react'
import type { ReactNode } from 'react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

const { setPtt, setSidebandOverride, pushToast } = vi.hoisted(() => ({
  setPtt: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  pushToast: vi.fn(),
}))

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({})),
  setPtt,
  setTxEnabled: vi.fn(async () => ({})),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride,
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))
vi.mock('../toast', () => ({
  pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The header renders the mode picker it is handed, and nothing else of its own.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ modeIndicator }: { modeIndicator?: ReactNode }) => (
    <header className="cockpit-header">{modeIndicator}</header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

afterEach(() => {
  cleanup()
  setPtt.mockClear()
  setSidebandOverride.mockClear()
  pushToast.mockClear()
})

const RULE = '60 m allows only USB phone'
const LOCK_TITLE = 'TX locked — 60 m allows only USB phone (pick USB or AUTO)'
const LOCK_TOAST = 'TX locked — 60 m allows only USB phone. Pick USB or AUTO.'
const FREQUENCY_TITLE =
  'TX locked — outside your license privileges (pick a band, or change your license in Settings)'
const FREQUENCY_TOAST = 'TX locked — this frequency/mode is outside your license privileges'

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 5.36,
      band: '60m',
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      rfPower: null,
      micGain: null,
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: null,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

const renderPhone = (over: Record<string, unknown> = {}) =>
  render(<PhoneCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} />)

/** The picker's buttons by the mode each names; AUTO's plate carries its sideband. */
function picks(): Record<string, HTMLButtonElement> {
  const out: Record<string, HTMLButtonElement> = {}
  for (const b of document.querySelectorAll<HTMLButtonElement>('.ph-mode-pick button')) {
    const text = b.textContent ?? ''
    out[/^AUTO/.test(text) ? 'AUTO' : text] = b
  }
  return out
}

const ptt = () => document.querySelector('.ph-ptt') as HTMLButtonElement

describe('the mode picker on 60 m', () => {
  it('greys LSB, AM and FM with the rule as their tooltip; AUTO and USB stay', () => {
    renderPhone({ phoneUsbOnly: true })
    const p = picks()
    expect(Object.keys(p).sort()).toEqual(['AM', 'AUTO', 'FM', 'LSB', 'USB'])
    const seen = Object.fromEntries(
      Object.entries(p).map(([m, b]) => [m, { disabled: b.disabled, title: b.getAttribute('title') }]),
    )
    expect(seen.LSB).toEqual({ disabled: true, title: RULE })
    expect(seen.AM).toEqual({ disabled: true, title: RULE })
    expect(seen.FM).toEqual({ disabled: true, title: RULE })
    expect(seen.USB).toEqual({ disabled: false, title: 'Force USB until you change bands' })
    expect(seen.AUTO.disabled).toBe(false)
    expect(seen.AUTO.title).not.toBe(RULE)
  })

  it('a greyed pick sends nothing; USB still does', async () => {
    renderPhone({ phoneUsbOnly: true })
    for (const m of ['LSB', 'AM', 'FM']) fireEvent.click(picks()[m])
    expect(setSidebandOverride).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.click(picks().USB)
    })
    expect(setSidebandOverride).toHaveBeenCalledWith('USB')
  })

  it('off 60 m every pick is offered as before', () => {
    renderPhone({ dialMhz: 14.286, band: '20m' })
    const p = picks()
    for (const m of ['USB', 'LSB', 'AM', 'FM']) {
      expect({ m, disabled: p[m].disabled, title: p[m].getAttribute('title') }).toEqual({
        m,
        disabled: false,
        title: `Force ${m} until you change bands`,
      })
    }
  })
})

describe('the lock names the rule when the mode is the whole reason', () => {
  it('the PTT tooltip and the toast a press raises say 60 m allows only USB phone', () => {
    renderPhone({ txAllowed: false, phoneUsbOnly: true, phoneModeRefused: true, sidebandOverride: 'AM' })
    expect(ptt().textContent).toMatch(/TX LOCKED/)
    expect(ptt().getAttribute('title')).toBe(LOCK_TITLE)
    fireEvent.keyDown(window, { code: 'Space' })
    expect(pushToast).toHaveBeenCalledWith(LOCK_TOAST, 'info', 3500)
    expect(setPtt).not.toHaveBeenCalledWith(true)
  })

  it('a frequency refusal keeps the frequency sentence', () => {
    renderPhone({ txAllowed: false, phoneUsbOnly: true, dialMhz: 5.34, sidebandOverride: 'AM' })
    expect(ptt().getAttribute('title')).toBe(FREQUENCY_TITLE)
    fireEvent.keyDown(window, { code: 'Space' })
    expect(pushToast).toHaveBeenCalledWith(FREQUENCY_TOAST, 'info', 3500)
  })

  it("the connection's own refusal still comes first", () => {
    const why = 'This connection is receive and control only.'
    renderPhone({ txAllowed: false, txRefusal: why, phoneUsbOnly: true, phoneModeRefused: true })
    expect(ptt().getAttribute('title')).toBe(why)
    fireEvent.keyDown(window, { code: 'Space' })
    expect(pushToast).toHaveBeenCalledWith(why, 'info', 3500)
  })
})
