// @vitest-environment jsdom
//
// THE CONTEST ENTRY'S BOXES ARE THE ENTRY'S OWN SIZE IN EVERY COCKPIT.
//
// THE DEFECT. Every entry box carries `settings-input le-fd-input`. Both rules were one class, and
// `.settings-input` (14 px, touch height) sits thousands of lines BELOW `.le-fd-input` (30 px, 58 px),
// so it won both properties on source order: the cockpits' "enlarged" Call / exchange boxes lettered
// at 14 px. Only the logger window looked right, because its `.contest-logger-scaled .le-fd-input` is
// two classes.
//
// WHY IT IS COMPUTED, NOT GREPPED. A regex for `.le-fd-input {` and `30px` passed the broken sheet: the
// declaration was there and lost. So this mounts the REAL cockpits with the REAL log form in contest
// mode, takes the boxes they render, resolves every rule of the real sheet against them with jsdom's
// selector engine, and picks the winner the way the cascade does. jsdom computes no style and lays
// nothing out, so what the boxes measure in Chrome is the layout harness's job, not this file's.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import type { ReactElement } from 'react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { parseRules, cmpSpec, type Rule } from './cssCascade'
import { PhoneCockpit } from './components/PhoneCockpit'
import { CwCockpit } from './components/CwCockpit'
import { RttyCockpit } from './components/RttyCockpit'
import { PskCockpit } from './components/PskCockpit'
import { Js8Cockpit } from './components/Js8Cockpit'
import { DetachedPanel } from './DetachedPanel'
import type { AppSnapshot, FieldDayStatus } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

// THE BUDGET. Five whole cockpits and the logger window mount here; a loaded full suite on this box
// has run cases many times slower than one core. 20 s holds that; a test that hangs still fails.
vi.setConfig({ testTimeout: 20_000 })

// Comments stripped BEFORE parsing (parseRules is brace-aware, not comment-aware). Both sheets, in the
// order the bundle imports them. Resolved from the cwd: under jsdom `import.meta.url` is an http URL.
const RULES: Rule[] = ['styles.css', 'cockpit-panes.css'].flatMap((f) =>
  parseRules(readFileSync(resolve(process.cwd(), 'src', f), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')),
)

/** The winning declaration of `prop` on `el` by the real cascade: `!important`, then specificity,
 *  then source order, over every rule jsdom's selector engine says reaches the element. */
function winner(el: Element, prop: string): { value: string; selector: string } | null {
  let best: { value: string; important: boolean; rule: Rule } | null = null
  for (const rule of RULES) {
    let hit = false
    try {
      hit = el.matches(rule.selector)
    } catch {
      hit = false // a pseudo-element or a selector jsdom cannot evaluate styles no plain element
    }
    if (!hit) continue
    for (const d of rule.decls) {
      if (d.prop !== prop) continue
      const important = /!\s*important\s*$/.test(d.value)
      const cand = { value: d.value.replace(/!\s*important\s*$/, '').trim(), important, rule }
      const beats =
        !best ||
        (important !== best.important
          ? important
          : cmpSpec(rule.spec, best.rule.spec) > 0 || (cmpSpec(rule.spec, best.rule.spec) === 0 && rule.order > best.rule.order))
      if (beats) best = cand
    }
  }
  return best ? { value: best.value, selector: best.rule.selector } : null
}

const FD = {
  running: false,
  state: 'Idle',
  qsoCount: 0,
  sections: 0,
  points: 0,
  event: 'arrlfd',
  log: [],
  receives: [
    { key: 'CLASS', kind: 'fdClass', required: true },
    { key: 'SECTION', kind: 'oneOf', required: true, domains: ['fd_sections'] },
  ],
  composing: [
    { key: 'CLASS', raw: '2A' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
} as unknown as FieldDayStatus

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  hunt: null,
  fieldDay: FD,
  radio: { dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

beforeEach(() => {
  localStorage.clear()
  document.documentElement.dataset.theme = 'dark'
  // Every backend call answered from here, so each view mounts its real log form unmocked.
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string): Promise<T> => {
      if (cmd === 'get_snapshot') return { ...snap, link: { tier: 'FT8' }, stations: [], conversations: [], activePeer: null, contestEntry: { rev: 1, call: '', fields: {}, marks: null } } as T
      if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'arrlfd' } as T
      if (/^(get_band_plan|get_licensed_band_plan|get_need_alerts|get_scp_calls|contest_removed|contest_unmerged|get_voice_messages)$/.test(cmd)) return [] as T
      return null as T
    },
  }
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

const settle = () => act(async () => new Promise((r) => setTimeout(r, 50)))

const COCKPITS: [string, () => ReactElement][] = [
  ['Phone', () => <PhoneCockpit snap={snap} fieldDay={FD} theme="dark" spots={[]} onWorkSpot={() => {}} />],
  ['CW', () => <CwCockpit snap={snap} fieldDay={FD} theme="dark" spots={[]} onWorkSpot={() => {}} />],
  ['RTTY', () => <RttyCockpit snap={snap} onSetTxEnabled={() => {}} />],
  ['PSK', () => <PskCockpit snap={snap} onSetTxEnabled={() => {}} />],
  ['JS8', () => <Js8Cockpit snap={snap} onSetTxEnabled={() => {}} />],
]

describe("the contest entry's boxes win their own size over `.settings-input`", () => {
  it.each(COCKPITS)('%s: the Call and exchange boxes letter at 30 px and stand 58 px tall', async (_name, mount) => {
    render(mount())
    await settle()
    const call = document.querySelector('[data-pane="log"] .le-fd-input-call')
    const codes = [...document.querySelectorAll('[data-pane="log"] .le-fd-input-code')]
    expect(call, 'the cockpit drew no contest entry in its log pane').not.toBeNull()
    expect(codes.length, 'the entry drew no exchange boxes').toBeGreaterThan(0)
    for (const box of [call!, ...codes]) {
      // The competitor is really on the box — otherwise this would pass with nothing to beat.
      expect(box.classList.contains('settings-input')).toBe(true)
      expect(winner(box, 'font-size')?.value, `${box.className}: the entry's type size lost`).toBe('calc(30px * var(--text-scale))')
      expect(winner(box, 'min-height')?.value, `${box.className}: the entry's height lost`).toBe('58px')
    }
    // The width stays where it was: the exchange boxes keep their narrow one, the Call box fills.
    for (const box of codes) expect(winner(box, 'width')?.value).toBe('130px')
    expect(winner(call!, 'width')?.value).toBe('100%')
  })

  it("the logger window's own bigger boxes still win there — the control that the fix did not flatten them", async () => {
    render(<DetachedPanel panel="contestlog" />)
    await act(async () => new Promise((r) => setTimeout(r, 400)))
    const call = document.querySelector('.contest-logger-scaled .le-fd-input-call')
    expect(call, 'the logger window drew no entry').not.toBeNull()
    expect(winner(call!, 'font-size')?.value).toBe('calc(40px * var(--text-scale))')
    expect(winner(call!, 'min-height')?.value).toBe('1.5em')
  })
})
