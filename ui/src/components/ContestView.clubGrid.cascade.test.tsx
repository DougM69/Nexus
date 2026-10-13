// @vitest-environment jsdom
//
// THE CLUB GRID'S LAYOUT RULES, as the cascade resolves them over the real sheets (the testkit's
// importance → specificity → source order, jsdom's own selector engine). jsdom lays nothing out, so
// this holds the RULES that make the layout, and the real-browser screenshots hold what they paint:
//
//   • the grid scrolls sideways inside itself when the bands do not fit, so the page never does;
//   • its mode heads stay in view while it scrolls, on a face as opaque as the window's own;
//   • each name is the containing block for the word a screen reader hears, so that word can never
//     lengthen the page from outside every scroller (the POTA list's 4431 px page at 360×740);
//   • every band has the same column, floored, so an empty one reads as free; past the floors the
//     grid scrolls (Chrome measured Field Day's ten bands fitting 1024 and New York's seventeen
//     scrolling inside the grid, the page 1024 wide either way);
//   • the band heads keep to one line; a name wraps between its words, inside one only past its
//     column, rather than spill into the next band;
//   • the pop-out sets the grid in the glance type the rest of that window has.
import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { loadSheets, css, resolveVal } from '../cssCascade.testkit'
import { MODES, chainOf, expandWith, parseRules, rgbHex, toRgb, tokensAt, winnerAt, type Mode } from '../cssCascade'
import { FdClubSection } from './ContestView'
import type { FdClubBoardRow, FdClubStatus } from '../types'

const row = (posid: string, posName: string, extra: Partial<FdClubBoardRow>): FdClubBoardRow => ({
  posid,
  posName,
  band: '20m',
  mode: 'CW',
  operator: 'W9ABC',
  qsos: 10,
  rate: 4,
  lastSeenSecs: 1,
  ...extra,
})
const HOST: FdClubStatus = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'W9ABC Field Day',
  hostCall: 'W9ABC',
  score: 0,
  qsos: 0,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  board: [
    row('aaaa0001', 'Comms Trailer', { band: '20m', mode: 'PH', link: 'host' }),
    row('bbbb0002', 'CW tent', { band: '40m', mode: 'CW', link: 'connected', heardSecs: 3 }),
    row('cccc0003', 'GOTA', { band: '6m', mode: 'PH', link: 'lost', heardSecs: 250 }),
  ],
}

beforeAll(() => loadSheets())

// The theme-aware resolver, for what the testkit cannot answer: its token table is the first plain
// `:root` block, which holds no palette, so a face is read here per mode instead.
const RULES = parseRules(
  readFileSync(resolve(process.cwd(), 'src/styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' ')),
)
/** An element's own face under `mode`, as hex: its background's winner, laid over black, so a face
 *  that lets anything through reads as a different colour from an opaque one. */
function faceHex(el: Element, mode: Mode): string | null {
  const chain = chainOf(el)
  const w = winnerAt(RULES, mode, chain, 'background', 'background-color')
  if (!w) return null
  const c = toRgb(expandWith(tokensAt(RULES, mode, chain), w.value), [0, 0, 0])
  return c ? rgbHex(c) : w.value
}
afterEach(() => cleanup())

/** The board in the host it is drawn in: the Contest screen's panel (the main window, the logger
 *  window and Remote all draw it there), or the pop-out window's own. */
function inHost(host: 'panel' | 'popout') {
  const { container } = render(
    host === 'panel' ? (
      <section className="conversation panel fieldday">
        <FdClubSection club={HOST} />
      </section>
    ) : (
      <div className="app detached">
        <FdClubSection club={HOST} detached />
      </div>
    ),
  )
  const q = (sel: string) => container.querySelector(sel) as HTMLElement
  return { face: container.firstElementChild as HTMLElement, q }
}

describe('the club grid scrolls inside itself and never widens the page', () => {
  it('its own box is the sideways scroller, as wide as the club block and no wider', () => {
    const { q } = inHost('panel')
    const scroller = q('.club-grid')
    expect(scroller.firstElementChild?.tagName).toBe('TABLE')
    expect(css(scroller, 'overflow-x')).toBe('auto')
    expect(parseFloat(css(scroller, 'min-width') ?? '')).toBe(0)
    expect(css(scroller, 'max-width')).toBe('100%')
    // The table fills the block in a wide window; past its content it widens only the scroller.
    expect(css(q('.club-grid-table'), 'width')).toBe('100%')
  })

  it('keeps its mode heads in view while it scrolls, on the opaque face of the window it is in, in every mode', () => {
    for (const host of ['panel', 'popout'] as const) {
      const { face, q } = inHost(host)
      const heads = [...face.querySelectorAll<HTMLElement>('.club-grid-mode')]
      expect(heads.length, `${host}: the corner and three mode heads`).toBe(4)
      expect(q('.club-grid-mode').closest('.club-grid'), `${host}: inside the scroller`).not.toBeNull()
      for (const h of heads) {
        expect(css(h, 'position'), host).toBe('sticky')
        expect(parseFloat(css(h, 'left') ?? ''), host).toBe(0)
      }
      const off: string[] = []
      for (const mode of MODES) {
        const ground = faceHex(face, mode)
        if (!ground?.startsWith('#')) off.push(`${host} ${mode}: no face on the window (${ground})`)
        for (const h of heads) {
          const own = faceHex(h, mode)
          if (own !== ground) off.push(`${host} ${mode}: ${h.textContent} on ${own}, the window ${ground}`)
        }
      }
      expect(off).toEqual([])
      cleanup()
    }
  }, 60_000)

  it('every name holds its screen-reader word in its own box, and so does the scroller its caption', () => {
    const { face, q } = inHost('panel')
    const names = [...face.querySelectorAll<HTMLElement>('.club-grid-pos')]
    expect(names.length, 'three names and four legend keys').toBe(7)
    for (const n of names) expect(css(n, 'position'), n.textContent ?? '').toBe('relative')
    expect(css(q('.club-grid'), 'position')).toBe('relative')
    expect(q('.club-grid caption').className).toBe('sr-only')
  })

  it('gives every band the same column, floored, and the mode heads one of their own', () => {
    const { face, q } = inHost('panel')
    expect(css(q('.club-grid-table'), 'table-layout')).toBe('fixed')
    // The fixed layout takes its columns from the first row: the corner and each band's head.
    const heads = [...face.querySelectorAll<HTMLElement>('thead th')]
    expect(heads.map((h) => h.className)).toEqual(['club-grid-mode', ...Array(10).fill('club-grid-band')])
    expect(new Set(heads.slice(1).map((h) => css(h, 'width')))).toEqual(new Set(['6.75em']))
    expect(css(heads[0], 'width')).toBe('5em')
    // A name fills its column, so the colour of a cell reads from across a room.
    expect(css(q('.club-grid-cell > .club-grid-pos'), 'display')).toBe('block')
  })

  it('keeps a band head on one line, and wraps a name between its words rather than into the next band', () => {
    const { q } = inHost('panel')
    expect(css(q('.club-grid-band'), 'white-space')).toBe('nowrap')
    expect(css(q('.club-grid-mode'), 'white-space')).toBe('nowrap')
    expect(css(q('.club-grid-pos'), 'white-space')).toBe('normal')
    expect(css(q('.club-grid-pos'), 'overflow-wrap')).toBe('anywhere')
    // The break point between a mark and its name is a real space: with none the two were one word.
    expect(q('.link-connected .club-grid-mark').textContent).toBe('✓ ')
  })

  it('sets the grid in the pop-out’s glance type, larger than the docked board’s', () => {
    const docked = inHost('panel').q('.club-grid-table')
    const dockedPx = parseFloat(resolveVal(css(docked, 'font-size') ?? ''))
    cleanup()
    const glance = inHost('popout').q('.club-grid-table')
    const glancePx = parseFloat(resolveVal(css(glance, 'font-size') ?? ''))
    expect(dockedPx).toBe(16)
    expect(glancePx).toBe(20)
  })
})
