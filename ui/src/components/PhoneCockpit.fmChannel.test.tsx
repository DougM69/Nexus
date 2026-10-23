// @vitest-environment jsdom
//
// A KEPT FM CHANNEL READS FM IN THE PHONE COCKPIT (the rule: a kept FM channel
// keeps commanding FM in Phone).
//
// Phone pressed on the SSTV screen on 144.500, the 2 m FM image channel, keeps the dial, and the
// engine now goes on commanding FM there (`fmChannel` in the snapshot) instead of Phone's own
// policy, USB for a station whose phone mode is SSB. The cockpit's AUTO face is its own copy of
// that policy, so without the snapshot's word it said AUTO·USB over a rig the engine had put in
// FM, and flagged the rig's correct FM read-back as a mismatch ("rig: FM").
//
// This suite asserts what the operator is SHOWN: the AUTO plate and the mismatch chip. The
// engine's choice of mode is pinned in Rust (`engine::mode_entry::tests`).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import type { ReactNode } from 'react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => ({})),
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
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The header renders the mode picker it is handed AND its children, where the `rig: …` mismatch
// chip lives, so the stub passes both through.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { children?: ReactNode; modeIndicator?: ReactNode }) => (
    <header className="cockpit-header">
      {p.modeIndicator}
      {p.children}
    </header>
  ),
}))
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

afterEach(cleanup)

/** Phone on the 2 m FM image channel, the rig reading back FM. */
function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 144.5,
      band: '2m',
      catOk: true,
      sideband: 'FM',
      sidebandOverride: null,
      rigMode: 'FM',
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

const renderPhone = (over: Record<string, unknown> = {}, phoneMode = 'ssb') =>
  render(
    <PhoneCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} phoneMode={phoneMode} />,
  )

/** What the AUTO button prints: the word and the mode it resolved to. */
const autoPlate = () =>
  [...document.querySelectorAll<HTMLButtonElement>('.ph-mode-pick button')]
    .map((b) => b.textContent ?? '')
    .find((t) => t.startsWith('AUTO'))

/** The `rig: …` chips the header shows (the mic-off-for-DAX chip shares the class). */
const mismatch = () =>
  [...document.querySelectorAll('.ph-mode-mismatch')].map((c) => c.textContent).filter((t) => t?.startsWith('rig:'))

describe('the Phone cockpit on a kept FM channel', () => {
  it('reads AUTO·FM and raises no mismatch over the rig’s FM', () => {
    renderPhone({ fmChannel: true })
    expect(autoPlate()).toBe('AUTO·FM')
    expect(mismatch()).toEqual([])
  })

  it('CONTROL: without the kept channel, an SSB station’s AUTO on 2 m is USB, and FM on the rig is flagged', () => {
    renderPhone({ rigMode: 'FM' })
    expect(autoPlate()).toBe('AUTO·USB')
    expect(mismatch()).toEqual(['rig: FM'])
  })

  it('CONTROL: a station whose phone mode is FM reads AUTO·FM as before', () => {
    renderPhone({}, 'fm')
    expect(autoPlate()).toBe('AUTO·FM')
    expect(mismatch()).toEqual([])
  })

  it('CONTROL: HF reads the band’s sideband as before', () => {
    renderPhone({ dialMhz: 14.23, band: '20m', sideband: 'USB', rigMode: 'USB' })
    expect(autoPlate()).toBe('AUTO·USB')
    cleanup()
    renderPhone({ dialMhz: 7.171, band: '40m', sideband: 'LSB', rigMode: 'LSB' })
    expect(autoPlate()).toBe('AUTO·LSB')
  })

  it('CONTROL: the operator’s own pick is what is commanded, kept channel or not', () => {
    renderPhone({ fmChannel: true, sidebandOverride: 'USB', rigMode: 'USB' })
    expect(mismatch()).toEqual([])
  })
})
