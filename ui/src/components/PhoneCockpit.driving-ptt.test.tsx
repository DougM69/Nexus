// @vitest-environment jsdom
//
// A COMPUTER DRIVING A STATION HOLDS ITS PTT BY RE-ASSERTING IT. Each `set_ptt` on from this
// screen is one hold of a held press on the station's Stop road (client mode), so while the PTT is
// down the screen's own timer sends one every 100 ms, as the stream page's does, and the release
// stops it: the station ends the press 200 ms after the last, so a hung screen cannot leave its
// over on the air. Lock is refused, as over the stream. A press the station refused or ended
// reads Over ended until it is let go. CONTROL: at the shack's own screen one press is one call.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, fireEvent, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { EN } from '../i18n/en'

const { setPtt, pushToast, driving } = vi.hoisted(() => ({
  setPtt: vi.fn(async (_on: boolean) => ({})),
  pushToast: vi.fn(),
  driving: { value: true },
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
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))
vi.mock('../toast', () => ({
  pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('../features/durableStore', async (actual) => ({
  ...(await actual<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => driving.value,
}))

vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => {
  vi.useFakeTimers()
  driving.value = true
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  setPtt.mockReset()
  setPtt.mockImplementation(async () => ({}))
  pushToast.mockClear()
})

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2,
      band: '20m',
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
    },
  } as unknown as AppSnapshot
}

const renderPhone = () =>
  render(<PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)

const ptt = () => document.querySelector('.ph-ptt') as HTMLButtonElement
const holds = () => setPtt.mock.calls.filter(([on]) => on === true).length

async function step(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms)
  })
}

describe('the Phone PTT on a computer driving a station', () => {
  it('re-asserts the press every 100 ms from its own timer while held, and stops at the release', async () => {
    renderPhone()
    await act(async () => {
      fireEvent.pointerDown(ptt())
    })
    expect(holds(), 'the first hold goes at the press').toBe(1)
    await step(350)
    expect(holds()).toBe(4)
    await act(async () => {
      fireEvent.pointerUp(ptt())
    })
    expect(setPtt).toHaveBeenLastCalledWith(false)
    await step(500)
    expect(holds(), 'a hold after the release').toBe(4)
    expect(ptt().textContent).toMatch(/PUSH TO TALK/)
  })

  it('a press the station refused or ended stops re-asserting and reads Over ended until let go', async () => {
    const words = EN['client.refused.pressEnded']
    setPtt.mockImplementation(async (on: boolean) => {
      if (on) throw words
      return {}
    })
    renderPhone()
    await act(async () => {
      fireEvent.pointerDown(ptt())
    })
    await step(10)
    expect(ptt().textContent).toMatch(/Over ended — release PTT/)
    expect(pushToast).toHaveBeenCalledWith(words, 'info', 4000)
    await step(500)
    expect(holds(), 'it kept re-asserting a press that ended').toBe(1)
    await act(async () => {
      fireEvent.pointerUp(ptt())
    })
    expect(ptt().textContent).toMatch(/PUSH TO TALK/)
    setPtt.mockImplementation(async () => ({}))
    await act(async () => {
      fireEvent.pointerDown(ptt())
    })
    await step(250)
    expect(holds(), 'a new press after the release').toBe(4)
    expect(ptt().textContent).toMatch(/ON AIR|Armed/)
  })

  it('Lock is refused, as over the stream', async () => {
    renderPhone()
    await act(async () => {
      fireEvent.click(document.querySelector('.ph-lock input') as HTMLInputElement)
    })
    await act(async () => {
      fireEvent.pointerDown(ptt())
    })
    await step(300)
    expect(holds()).toBe(0)
    expect(pushToast).toHaveBeenCalledWith(EN['phone.tx.remoteLock'], 'info', 4000)
  })

  it('control: at the shack’s own screen one press is one call', async () => {
    driving.value = false
    renderPhone()
    await act(async () => {
      fireEvent.pointerDown(ptt())
    })
    await step(350)
    expect(holds()).toBe(1)
  })
})
