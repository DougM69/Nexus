// @vitest-environment jsdom
//
// THE RTTY TONE BAND — a shaded band between mark and space, where the decoder is listening, in
// `--rx`; in `--tx` while we are keyed on those tones.
//
// The recording-canvas technique is Waterfall.width.test.tsx's, and faithful for the same reason:
// the overlay only WRITES to its context, so the component's own effect, rAF loop and drawOverlay
// run as shipped. A 1000 × 200 box at text scale 1 leaves a 182 px waterfall over the 18 px axis
// strip, and a fixed 1000–2000 Hz window maps one hertz to one pixel, so a band's pixels are its
// frequencies minus 1000.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Waterfall } from './Waterfall'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getSpectrumRow: () => new Promise(() => {}),
}))

type Call = { op: string; args: unknown[]; fillStyle: string; alpha: number }

function recordingCtx() {
  const calls: Call[] = []
  const state: Record<string, unknown> = { fillStyle: '#000000', globalAlpha: 1, font: '', textBaseline: 'alphabetic' }
  const ctx = new Proxy(state, {
    get(target, prop) {
      if (typeof prop !== 'string') return undefined
      if (prop in target) return target[prop]
      if (prop === 'measureText') return () => ({ width: 10 })
      return (...args: unknown[]) => {
        calls.push({ op: prop, args, fillStyle: String(target.fillStyle), alpha: Number(target.globalAlpha) })
      }
    },
    set(target, prop, value) {
      if (typeof prop === 'string') target[prop] = value
      return true
    },
  })
  return { ctx, calls }
}

const TX = '#a1b2c3'
const RX = '#c3b2a1'
const MARK = '#0a0b0c'
const SPACE = '#0c0b0a'
const W = 1000
const H = 200
const WF_H = 182
const VIEW = { lo: 1000, hi: 2000 }
const CURSORS = [
  { hz: 1415, color: MARK, label: 'M' },
  { hz: 1585, color: SPACE, label: 'S' },
]

let overlay: ReturnType<typeof recordingCtx>
let realRaf: typeof requestAnimationFrame
let realCaf: typeof cancelAnimationFrame

beforeEach(() => {
  localStorage.clear()
  overlay = recordingCtx()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    return (this.classList.contains('waterfall-overlay') ? overlay.ctx : null) as unknown as CanvasRenderingContext2D
  } as unknown as typeof HTMLCanvasElement.prototype.getContext)
  vi.spyOn(HTMLCanvasElement.prototype, 'getBoundingClientRect').mockImplementation(
    () => ({ left: 0, top: 0, right: W, bottom: H, width: W, height: H, x: 0, y: 0, toJSON() {} }) as DOMRect,
  )
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  realRaf = globalThis.requestAnimationFrame
  realCaf = globalThis.cancelAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) =>
    setTimeout(() => cb(performance.now()), 16) as unknown as number) as typeof requestAnimationFrame
  globalThis.cancelAnimationFrame = ((id: number) => clearTimeout(id as unknown as NodeJS.Timeout)) as typeof cancelAnimationFrame
  for (const [k, v] of Object.entries({ '--tx': TX, '--rx': RX, '--well-bg': '#010203', '--well-ink': '#fefdfc' }))
    document.documentElement.style.setProperty(k, v)
})

afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  globalThis.cancelAnimationFrame = realCaf
  document.documentElement.removeAttribute('style')
  localStorage.clear()
  vi.restoreAllMocks()
})

const frames = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))

function lastFrame(): Call[] {
  const at = overlay.calls.map((c) => c.op).lastIndexOf('clearRect')
  return overlay.calls.slice(at)
}
/** Full-height fills wider than a cursor line: [ink, x, width]. */
const bands = (f: Call[]) =>
  f
    .filter((c) => c.op === 'fillRect' && c.args[1] === 0 && c.args[3] === WF_H && (c.args[2] as number) > 2)
    .map((c) => [c.fillStyle, c.args[0], c.args[2]])

function wf(band: { loHz: number; hiHz: number } | undefined, keyed = false) {
  return (
    <Waterfall
      transmitting={false}
      rxOffsetHz={1500}
      txOffsetHz={0}
      theme="dark"
      fixedWindow={VIEW}
      cursors={CURSORS}
      toneBand={band}
      keyed={keyed}
    />
  )
}

describe('the RTTY tone band', () => {
  it('shades mark to space in --rx, exactly where the tones are', async () => {
    render(wf({ loHz: 1415, hiHz: 1585 }))
    await frames(120)
    expect(bands(lastFrame())).toEqual([[RX, 415, 170]])
  })

  it('follows the tones when they move (a net, or AFC)', async () => {
    const { rerender } = render(wf({ loHz: 1415, hiHz: 1585 }))
    await frames(120)
    rerender(wf({ loHz: 1527, hiHz: 1697 }))
    await frames(120)
    expect(bands(lastFrame())).toEqual([[RX, 527, 170]])
  })

  it('is drawn under the cursors, so it never hides a tone line', async () => {
    render(wf({ loHz: 1415, hiHz: 1585 }))
    await frames(120)
    const f = lastFrame()
    const band = f.findIndex((c) => c.op === 'fillRect' && c.fillStyle === RX && c.args[2] === 170)
    const mark = f.findIndex((c) => c.op === 'fillRect' && c.fillStyle === MARK)
    expect(band).toBeGreaterThanOrEqual(0)
    expect(mark).toBeGreaterThan(band)
  })

  it('turns --tx while we are keyed, and only then', async () => {
    const { rerender } = render(wf({ loHz: 1415, hiHz: 1585 }, true))
    await frames(120)
    const keyed = bands(lastFrame())
    expect(keyed).toEqual([[TX, 415, 170]])
    rerender(wf({ loHz: 1415, hiHz: 1585 }, false))
    await frames(120)
    expect(bands(lastFrame()), 'the keyed tint outlived the over').toEqual([[RX, 415, 170]])
  })

  it('is clipped to the view, and a band out of view draws nothing', async () => {
    const { rerender } = render(wf({ loHz: 1915, hiHz: 2085 }))
    await frames(120)
    expect(bands(lastFrame())).toEqual([[RX, 915, 85]])
    rerender(wf({ loHz: 2215, hiHz: 2385 }))
    await frames(120)
    expect(bands(lastFrame())).toEqual([])
  })

  it('control: no band unless asked for (every surface but RTTY)', async () => {
    render(wf(undefined, true))
    await frames(120)
    expect(bands(lastFrame())).toEqual([])
  })
})
