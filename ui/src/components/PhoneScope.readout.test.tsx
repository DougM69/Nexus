// @vitest-environment jsdom
//
// THE ▲dB READOUT HOLDS ITS PLACE IN THE ROW.
//
// An operator's video (2026-10-13): "the xxdb G Z ... line, keeps jittering because the db is
// changing between single and double digits … give it a fixed field". The readout led the scope's
// control row as a bare text run, so every change in its digit count moved G, Z and every button
// after it, and on a row just wide enough for a shorter reading a longer one wrapped it onto a second
// line, taking that height from the picture (CW at 1024×768, UI scale 175 %, measured in Chrome).
//
// THE RANGE THE FIELD MUST HOLD, read off the code: the readout is round((peak − floor) × dB per
// unit) with both 0..1 (spectrum/scale.ts), so it never passes the widest axis either way — 120 dB
// on the audio feed and on a radio's unpublished scale, 80 on a CI-V scope. And it is SIGNED: while
// keyed the floor is frozen and the row is not, so a receiver muted under TX reads below its own
// pre-TX floor. Both ends are driven here through the component's own loop, not assumed.
//
// WHAT THIS PINS, through the real cascade (cssCascade.testkit) on the rendered chip: the number
// stands alone in a box of its own (the ▲ and the unit never change, so they stay outside it), in a
// display that honours a width, whose winning `min-width` in `ch` is at least the widest value's
// length, in tabular figures.
//
// WHAT IT DOES NOT PROVE: any layout. That a `ch` holds a tabular digit and the minus sign is the
// font's business; it was measured in Chrome, with the controls after the chip standing still.
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { axisOf } from '../spectrum/scale'
import { css, loadSheets } from '../cssCascade.testkit'

// The house budget: the real sheets are parsed once per file, and a loaded suite runs slow.
vi.setConfig({ testTimeout: 15_000 })

/** The scripted source: every 4th bin at `peak`, the rest at `floor`. The readout measures from the
 *  MEDIAN, which is the floor, and every window it reads holds a peak bin. */
const src = { floor: 0.1, peak: 0.1, polls: 0 }

vi.mock('../api', () => ({
  getScopeFrame: () => {
    src.polls++
    return Promise.resolve({
      seq: src.polls,
      tMs: 1_700_000_000_000 + src.polls * 50,
      source: 'audio',
      loHz: 0,
      hiHz: 4000,
      scale: { kind: 'dbfs', loDb: -120, hiDb: 0 },
      bins: Array.from({ length: 512 }, (_, i) => (i % 4 === 0 ? src.peak : src.floor)),
    })
  },
}))

/** The widest the readout can print: |peak − floor| ≤ 1 on every axis the scope draws on, so the
 *  widest axis's span, negative. */
const WIDEST = String(
  -Math.max(
    axisOf({ kind: 'dbfs', loDb: -120, hiDb: 0 }, 'audio').dbPerUnit,
    ...['civ', 'flex', 'yaesu'].map((source) => axisOf({ kind: 'relative' }, source).dbPerUnit),
  ),
)

beforeAll(() => loadSheets())

beforeEach(() => {
  Object.assign(src, { floor: 0.1, peak: 0.1, polls: 0 })
  localStorage.clear()
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance', 'Date'] })
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', clearTimeout)
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.stubGlobal('ImageData', class {
    data: Uint8ClampedArray; width: number; height: number
    constructor(a: Uint8ClampedArray | number, b: number, c?: number) {
      if (typeof a === 'number') { this.width = a; this.height = b; this.data = new Uint8ClampedArray(a * b * 4) }
      else { this.data = a; this.width = b; this.height = c ?? a.length / 4 / b }
    }
  })
  // A 2D paint target only: `webgl2` answers null, so the renderer draws on its canvas-2D path.
  const paint = {
    fillRect() {}, clearRect() {}, putImageData() {}, beginPath() {}, closePath() {}, moveTo() {}, lineTo() {}, fillText() {},
    fill() {}, stroke() {}, setLineDash() {}, save() {}, restore() {}, rect() {}, clip() {},
    measureText: (t: string) => ({ width: t.length * 6 }), createLinearGradient: () => ({ addColorStop() {} }),
  }
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
    kind === '2d' ? paint : null) as unknown as HTMLCanvasElement['getContext'])
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 400, height: 200, right: 400, bottom: 200, toJSON() { return {} } })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

/** Run the scope's own loop for `ms` of fake time (a poll every 50 ms). */
async function run(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })
}

const chip = () => screen.getByTitle(/strongest signal in this view stands above the noise floor/)

/** The cascade winner of an INHERITED property: the element's own, else its nearest ancestor's. */
function inherited(el: Element, prop: string): string | null {
  for (let e: Element | null = el; e; e = e.parentElement) {
    const v = css(e, prop)
    if (v) return v
  }
  return null
}

/** The readout's number in a field of its own that holds `text` and every value wider than it. */
function expectFixedField(text: string) {
  const field = [...chip().querySelectorAll('*')].find((el) => el.textContent === text)
  expect(field, `"${text}" has no box of its own in "${chip().textContent}" — its width is the text run's`).toBeDefined()
  // Undeclared is the initial value: a span is inline, and figures are proportional.
  expect(css(field!, 'display') ?? 'inline', 'an inline box ignores min-width').toMatch(/^inline-(block|flex|grid)$/)
  const min = /^([\d.]+)ch$/.exec(css(field!, 'min-width') ?? '')
  expect(min, `the field's min-width is ${css(field!, 'min-width')}, not a width in characters`).not.toBeNull()
  expect(Number(min![1]), `the field must hold "${WIDEST}", the widest the readout prints`).toBeGreaterThanOrEqual(
    WIDEST.length,
  )
  expect(
    inherited(field!, 'font-variant-numeric') ?? 'normal',
    'proportional figures change width with the digits',
  ).toMatch(/tabular-nums/)
}

describe('the ▲dB readout is a fixed field', () => {
  it('holds the top of its range: a full-scale peak over a silent floor', async () => {
    Object.assign(src, { floor: 0, peak: 1 })
    render(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} />)
    await run(500)
    expect(src.polls, 'control: the loop never polled').toBeGreaterThan(3)
    expect(chip().textContent).toBe(`▲${WIDEST.slice(1)} dB`)
    expectFixedField(WIDEST.slice(1))
  })

  it('holds the bottom, sign included: keyed, the floor frozen at full scale and the row silent', async () => {
    // Settle the floor at full scale (peak = floor, so ▲0), key the rig, and let the row fall silent.
    Object.assign(src, { floor: 1, peak: 1 })
    const view = render(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} />)
    await run(1500)
    expect(chip().textContent, 'control: the floor never settled').toBe('▲0 dB')
    view.rerender(<PhoneScope transmitting theme="dark" viewLoHz={-1e9} viewHiHz={1e9} />)
    Object.assign(src, { floor: 0, peak: 0 })
    await run(500)
    expect(chip().textContent).toBe(`▲${WIDEST} dB`)
    expectFixedField(WIDEST)
  })

  it('keeps one field for the values in the operator video', async () => {
    for (const v of [8, 10]) {
      Object.assign(src, { floor: 0.1, peak: 0.1 + v / 120 })
      render(<PhoneScope transmitting={false} theme="dark" viewLoHz={-1e9} viewHiHz={1e9} />)
      await run(500)
      expect(chip().textContent).toBe(`▲${v} dB`)
      expectFixedField(String(v))
      cleanup()
    }
  })
})
