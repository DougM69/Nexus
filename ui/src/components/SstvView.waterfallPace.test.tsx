// @vitest-environment jsdom
//
// The SSTV band waterfall draws 50 ms rows at an even pace, as the RTTY one does: the real Waterfall,
// mounted in the cockpit, on a frame clock turned by hand. Without the kept remainder a 50 ms row
// takes 4 frames at 60 Hz (30 rows in 2 s, see Waterfall.rttyLook.test.tsx); with it, 3 or 4 frames
// in turn (40 rows).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, cleanup, waitFor } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, SstvState } from '../types'
import type { SpectrumRenderer } from '../spectrum'

vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(async () => []),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
  getSpectrumRow: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('../spectrum', async (importActual) => {
  const actual = await importActual<typeof import('../spectrum')>()
  return {
    ...actual,
    createSpectrumRenderer: (): SpectrumRenderer => ({
      backend: 'canvas2d',
      reason: '',
      canvas: null,
      rows: 0,
      resize() {},
      commitRow() {},
      rowAt: () => null,
      clearHistory() {},
      draw() {},
      destroy() {},
    }),
  }
})

const getSstvState = api.getSstvState as ReturnType<typeof vi.fn>
const getSpectrumRow = api.getSpectrumRow as ReturnType<typeof vi.fn>

const snap = {
  radio: {
    dialMhz: 14.23,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    txAllowed: true,
  },
} as unknown as AppSnapshot

const IDLE = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  hedrShiftHz: 0,
  gallery: [],
  health: { armed: false, audioPeak: 0, drains: 0, visSeen: 0, unknownVis: 0, images: 0 },
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
} as unknown as SstvState

// The waterfall's frame clock, turned by hand.
let clock = 0
let queue: FrameRequestCallback[] = []
let realRaf: typeof requestAnimationFrame
async function frames(n: number, ms: number) {
  for (let i = 0; i < n; i++) {
    clock += ms
    const due = queue
    queue = []
    await act(async () => {
      for (const cb of due) cb(clock)
      for (let k = 0; k < 5; k++) await Promise.resolve()
    })
  }
}

beforeEach(() => {
  localStorage.clear()
  clock = 1000
  queue = []
  getSstvState.mockReset().mockResolvedValue(IDLE)
  ;(api.sstvAutoArm as ReturnType<typeof vi.fn>).mockReset().mockResolvedValue(IDLE)
  getSpectrumRow.mockReset().mockResolvedValue({ row: [], loHz: 0, hiHz: 4000, source: 'audio' })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => null)
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  realRaf = globalThis.requestAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => queue.push(cb)) as typeof requestAnimationFrame
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  vi.restoreAllMocks()
})

describe('SSTV band waterfall pace', () => {
  it('draws 40 rows of 50 ms in 2 s at 60 Hz, not 30', async () => {
    vi.spyOn(performance, 'now').mockImplementation(() => clock)
    const { container } = render(<SstvView snap={snap} />)
    await waitFor(() => expect(container.querySelector('.waterfall-canvas')).not.toBeNull())
    await frames(2, 50 / 3)
    getSpectrumRow.mockClear()
    await frames(120, 50 / 3) // 2 s
    expect(getSpectrumRow.mock.calls.length).toBe(40)
  })
})
