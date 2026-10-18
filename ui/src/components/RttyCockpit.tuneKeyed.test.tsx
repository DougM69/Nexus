// @vitest-environment jsdom
//
// NO RETUNE WHILE KEYED — the RTTY waterfall's click and its Auto-tune button.
//
// A net moves the AFSK transmit tones as well as the decoder: the TX stream is built from the
// netted centre (#128, since 1.9.0). So a click on the waterfall mid-over slid the transmitter to
// wherever the pointer landed, in the middle of the over. Both ways of netting are refused while
// we are keyed (an over on the air or queued, the continuous-TX latch, a tune carrier), and the
// refusal re-reads the engine's state rather than trusting a poll that can be 500 ms old.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor, cleanup, fireEvent, act } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import * as api from '../api'
import * as toast from '../toast'
import type { AppSnapshot, RttyState } from '../types'
import { WF_DB_SPAN } from '../waterfall'

vi.setConfig({ testTimeout: 15_000 })

globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver

vi.mock('../api', () => ({
  getRttyState: vi.fn(),
  rttyArm: vi.fn(),
  rttyAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  rttySend: vi.fn(),
  rttySetLatched: vi.fn(),
  rttyType: vi.fn(),
  rttyStop: vi.fn(),
  rttyClear: vi.fn(),
  rttyAfcReset: vi.fn(),
  rttyNet: vi.fn(),
  haltTx: vi.fn(),
  getSpectrumRow: vi.fn(),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const getRttyState = api.getRttyState as ReturnType<typeof vi.fn>
const rttyAutoArm = api.rttyAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>
const getSpectrumRow = api.getSpectrumRow as ReturnType<typeof vi.fn>
const rttyNet = api.rttyNet as ReturnType<typeof vi.fn>
const pushToast = toast.pushToast as ReturnType<typeof vi.fn>

// Netted on 1400/1570 (centre 1485), 170 Hz shift: the signal in the spectrum below is at 1500.
const ARMED: RttyState = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: '',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  backend: 'afsk',
  sending: false,
  latched: false,
  keyerError: null,
  markHz: 1400,
  spaceHz: 1570,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
}

const snapWith = (over: Record<string, unknown>) =>
  ({
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.08,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: true,
      tuning: false,
      txAllowed: true,
      ...over,
    },
  }) as unknown as AppSnapshot

/** A producer row over 0–4000 Hz: noise at −95 dB and a lobe at each tone. */
function spectrum(tones: { hz: number; db: number }[]) {
  const w = 4000 / 512
  const db = Array.from({ length: 512 }, (_, i) => -95 + 1.5 * Math.sin(i * 1.7))
  for (const t of tones) {
    const at = t.hz / w - 0.5
    for (let i = Math.floor(at) - 3; i <= Math.ceil(at) + 3; i++) db[i] = Math.max(db[i], t.db - 12 * (i - at) ** 2)
  }
  return { row: db.map((d) => Math.max(0, 1 + d / WF_DB_SPAN)), loHz: 0, hiHz: 4000, source: 'audio' }
}
// The wanted 170 Hz pair at 1415/1585, a lone carrier, and a pair at 425 Hz shift.
const SCENE = spectrum([
  { hz: 1415, db: -40 },
  { hz: 1585, db: -42 },
  { hz: 1300, db: -35 },
  { hz: 1650, db: -38 },
  { hz: 2075, db: -38 },
])

beforeEach(() => {
  localStorage.clear()
  getRttyState.mockReset().mockResolvedValue(ARMED)
  rttyAutoArm.mockReset().mockImplementation(() => api.getRttyState())
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  getSpectrumRow.mockReset().mockResolvedValue(SCENE)
  rttyNet.mockReset().mockImplementation(async (hz: number) => ({ ...ARMED, markHz: hz - 85, spaceHz: hz + 85 }))
  pushToast.mockReset()
  // The one box the click is measured against: 1000 px over the default Std view, 200–3000 Hz.
  vi.spyOn(HTMLCanvasElement.prototype, 'getBoundingClientRect').mockImplementation(
    () => ({ left: 0, top: 0, right: 1000, bottom: 200, width: 1000, height: 200, x: 0, y: 0, toJSON() {} }) as DOMRect,
  )
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const settle = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))

async function mount(state: RttyState, snapOver: Record<string, unknown> = {}) {
  getRttyState.mockResolvedValue(state)
  const r = render(<RttyCockpit snap={snapWith(snapOver)} />)
  await waitFor(() => expect(r.container.querySelector('.waterfall-canvas')).not.toBeNull())
  await settle(50)
  return r
}
const canvas = (c: HTMLElement) => c.querySelector('.waterfall-canvas') as HTMLCanvasElement
const autoTuneBtn = () => screen.getByRole('button', { name: 'Auto-tune' })

describe('a waterfall click nets — and is ignored while we are keyed', () => {
  it('idle: a click at 500 px of a 200–3000 Hz view nets the decoder on 1600 Hz', async () => {
    const { container } = await mount(ARMED)
    fireEvent.mouseDown(canvas(container), { button: 0, clientX: 500 })
    await waitFor(() => expect(rttyNet).toHaveBeenCalledWith(1600))
    expect(rttyNet).toHaveBeenCalledTimes(1)
  })

  for (const [what, state, snapOver] of [
    ['an over on the air', { ...ARMED, sending: true }, {}],
    ['continuous TX latched', { ...ARMED, latched: true }, {}],
    ['a tune carrier', ARMED, { tuning: true }],
  ] as const) {
    it(`${what}: the click does nothing, and the hint says tuning is held`, async () => {
      const { container } = await mount(state, snapOver)
      await waitFor(() => expect(screen.getByText('tuning held while transmitting')).toBeTruthy())
      fireEvent.mouseDown(canvas(container), { button: 0, clientX: 500 })
      await settle(100)
      expect(rttyNet, 'a click moved the transmit tones mid-over').not.toHaveBeenCalled()
    })
  }

  it('an over that keyed since the last poll: the click re-reads the engine and does nothing', async () => {
    const { container } = await mount(ARMED)
    expect(screen.queryByText('tuning held while transmitting')).toBeNull()
    getRttyState.mockResolvedValue({ ...ARMED, sending: true })
    fireEvent.mouseDown(canvas(container), { button: 0, clientX: 500 })
    await settle(100)
    expect(rttyNet).not.toHaveBeenCalled()
  })
})

describe('Auto-tune', () => {
  it('nets onto the 170 Hz pair, past a lone carrier and a 425 Hz pair', async () => {
    await mount(ARMED)
    fireEvent.click(autoTuneBtn())
    await waitFor(() => expect(rttyNet).toHaveBeenCalledTimes(1), { timeout: 3000 })
    const hz = rttyNet.mock.calls[0][0] as number
    expect(Math.abs(hz - 1500), `netted on ${hz}`).toBeLessThan(4)
  })

  it('searches from where the decoder listens: the netted centre plus the AFC', async () => {
    // Netted on 2125/2295 (centre 2210), 700 Hz from the signal — out of reach — but the AFC
    // has pulled the decoder down to 1510, beside it.
    await mount({ ...ARMED, markHz: 2125, spaceHz: 2295, afcHz: -700 })
    fireEvent.click(autoTuneBtn())
    await waitFor(() => expect(rttyNet).toHaveBeenCalledTimes(1), { timeout: 3000 })
    expect(Math.abs((rttyNet.mock.calls[0][0] as number) - 1500)).toBeLessThan(4)
  })

  it('finds nothing: says so and nets nowhere', async () => {
    getSpectrumRow.mockResolvedValue(spectrum([{ hz: 1300, db: -35 }]))
    await mount(ARMED)
    fireEvent.click(autoTuneBtn())
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith('Auto-tune: no RTTY signal at this shift near the current tones', 'info'), {
      timeout: 3000,
    })
    expect(rttyNet).not.toHaveBeenCalled()
  })

  it('is disabled while keyed, and its press nets nowhere', async () => {
    await mount({ ...ARMED, sending: true })
    await waitFor(() => expect((autoTuneBtn() as HTMLButtonElement).disabled).toBe(true))
    fireEvent.click(autoTuneBtn())
    await settle(600)
    expect(rttyNet).not.toHaveBeenCalled()
  })

  it('an over that keys while it is sampling: it re-reads the engine and nets nowhere', async () => {
    await mount(ARMED)
    expect((autoTuneBtn() as HTMLButtonElement).disabled).toBe(false)
    let pressed = false
    getSpectrumRow.mockImplementation(async () => {
      if (pressed) getRttyState.mockResolvedValue({ ...ARMED, sending: true })
      return SCENE
    })
    pressed = true
    fireEvent.click(autoTuneBtn())
    await settle(900)
    expect(rttyNet, 'Auto-tune moved the transmit tones mid-over').not.toHaveBeenCalled()
    expect(pushToast).not.toHaveBeenCalled()
  })
})
