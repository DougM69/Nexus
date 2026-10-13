// THE ROTOR BOX IN THE DASHBOARD RAIL DRAWS THE BIGGEST ROSE ITS BOX HOLDS: the arithmetic (rotorFit.ts),
// by value. The parts are as Chrome measured them in the rail at Normal text: the bearing and elevation
// boxes 38.5 px and STOP 75, 4 px apart (160 on one line), 26 tall, and line 2 held at STOP's height; the hint
// two lines, 26; the bearing at 1.25em (17.5 px) and the elevation at 14; the pane grid's 8 px column and 4 px
// row gaps. The boxes are the rail's bodies less their padding and the held scrollbar gutter (a 300 px rail's
// body is 247 wide).
import { describe, it, expect } from 'vitest'
import { rotorFit, type RotorRoom } from './rotorFit'

const PARTS = { controlsW: 160, controlsH: 26, line2H: 26, hintH: 26, azMin: 17.5, elMin: 14, colGap: 8, rowGap: 4 }
const box = (w: number, h: number, over: Partial<RotorRoom> = {}): RotorRoom => ({ ...PARTS, w, h, ...over })

describe('the rotor box in the dashboard rail', () => {
  it('in the operator’s box (a 324 px rail, a 270 px body) draws a 157 px rose, the readings beside it and the boxes under', () => {
    expect(rotorFit(box(271, 255))).toEqual({ shape: 'under', room: 195, rose: 157, az: 31, el: 21 })
  })

  it('in the stock rail’s short box (139 px at 1366×768) keeps the boxes beside the rose, which grows only to the readings’ height', () => {
    // 63 px is under the 76 px the bearing, the elevation and the boxes take beside it: nothing moves down.
    expect(rotorFit(box(247, 123))).toEqual({ shape: 'beside', room: 63, rose: 63, az: 17, el: 14 })
  })

  it('the smaller wins: a taller box grows the rose until the width caps it, a wider one until the height does', () => {
    const taller = [183, 255, 436, 690].map((h) => rotorFit(box(247, h))!.rose)
    expect(taller, 'at 247 px wide').toEqual([93, 143, 143, 143])
    const wider = [247, 300, 480, 667].map((w) => rotorFit(box(w, 255))!.rose)
    expect(wider, 'at 255 px tall').toEqual([143, 165, 195, 195])
    // …and each cap is the other side's: the height left above line 2 and the hint (195), and three fifths
    // of the width beside the boxes and STOP under it (0.6 × 239 = 143).
    expect(rotorFit(box(667, 255))!.room).toBe(195)
  })

  it('takes the arrangement that draws the bearing bigger: beside in a short wide box, under in a tall one', () => {
    expect(rotorFit(box(667, 183))).toMatchObject({ shape: 'beside', rose: 123, az: 24 })
    expect(rotorFit(box(667, 590))).toMatchObject({ shape: 'under', rose: 395, az: 79 })
  })

  it('never draws a reading wider than the column beside the rose leaves', () => {
    for (const w of [147, 200, 247, 271, 300, 400, 480, 560, 667])
      for (const h of [97, 123, 183, 255, 436, 590, 690]) {
        const f = rotorFit(box(w, h))!
        const col = w - PARTS.colGap - f.rose
        // "123°T" is three of its font size wide and "EL 180°" 4.2: with room, 3.3 and 4.4.
        expect(f.az * 3.3, `the bearing at ${w}×${h}`).toBeLessThanOrEqual(col + 0.01)
        expect(f.el * 4.4, `the elevation at ${w}×${h}`).toBeLessThanOrEqual(col + 0.01)
        if (f.shape === 'beside') expect(col, `the boxes and STOP beside the rose at ${w}×${h}`).toBeGreaterThanOrEqual(PARTS.controlsW)
      }
  })

  it('never draws a reading smaller than the compact layout does', () => {
    for (const [w, h] of [[147, 97], [247, 123], [271, 255], [667, 590]]) {
      const f = rotorFit(box(w, h, { azMin: 21.875, elMin: 17.5 }))!
      expect(f.az, `the bearing at ${w}×${h}, Larger text`).toBeGreaterThanOrEqual(21)
      expect(f.el, `the elevation at ${w}×${h}, Larger text`).toBeGreaterThanOrEqual(17)
    }
  })

  it('holds the room line 2 takes: a line 2 on two lines takes its second line from the rose', () => {
    expect(rotorFit(box(271, 255, { line2H: 40 }))).toEqual({ shape: 'under', room: 181, rose: 151, az: 30, el: 21 })
  })

  it('holds nothing in a box that has not been laid out (hidden, or jsdom)', () => {
    expect(rotorFit(box(0, 0))).toBeNull()
    expect(rotorFit(box(247, 0))).toBeNull()
  })
})
