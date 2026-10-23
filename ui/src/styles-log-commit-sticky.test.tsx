// @vitest-environment jsdom
//
// THE COMMIT BLOCK STICKS TO THE BOTTOM OF EVERY COCKPIT'S LOG PANE.
//
// THE DEFECT. At 1280×800 the log form needs more height than the log pane gets in all five keyboard
// and voice cockpits, so Log sat below the fold and every contact needed a scroll first. The answer
// (operator, 2026-10-19) is the "logs to … as" line plus the Log/Spot row as ONE block that sticks to
// the bottom of the pane's own scroller. Bottom only: a top-sticky row would park Log over the recall
// card that follows it, which is why a sticky row was refused on 2026-08-04.
//
// WHY IT IS COMPUTED, NOT GREPPED. A declaration that is present and loses is the dead-fix class this
// repo has shipped twice. So this mounts the REAL cockpits with the REAL log form, takes the block they
// render, and resolves both sheets against it with jsdom's selector engine, picking the winner the way
// the cascade does. jsdom lays nothing out: that Log is really on screen, that nothing covers it and that
// the recall card is never under it are the layout harness's job, not this file's.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import type { ReactElement } from 'react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { parseRules, cmpSpec, type Rule } from './cssCascade'
import { PhoneCockpit } from './components/PhoneCockpit'
import { CwCockpit } from './components/CwCockpit'
import { RttyCockpit } from './components/RttyCockpit'
import { PskCockpit } from './components/PskCockpit'
import { Js8Cockpit } from './components/Js8Cockpit'
import { LogEntry } from './components/LogEntry'
import type { AppSnapshot } from './types'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

// Five whole cockpits mount here; a loaded full suite on this box runs cases many times slower than
// one core. 20 s holds that; a test that hangs still fails.
vi.setConfig({ testTimeout: 20_000 })

// Comments stripped BEFORE parsing (parseRules is brace-aware, not comment-aware). Both sheets, in the
// order the bundle imports them. Resolved from the cwd: under jsdom `import.meta.url` is an http URL.
const RULES: Rule[] = ['styles.css', 'cockpit-panes.css'].flatMap((f) =>
  parseRules(readFileSync(resolve(process.cwd(), 'src', f), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')),
)

/** The winning declaration of `prop` on `el` by the real cascade: `!important`, then specificity,
 *  then source order, over every rule jsdom's selector engine says reaches the element. */
function winner(el: Element, prop: string): string | null {
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
  return best ? best.value : null
}

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FT8',
  hunt: null,
  radio: { dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

beforeEach(() => {
  localStorage.clear()
  document.documentElement.dataset.theme = 'dark'
  // Every backend call answered from here, so each view mounts its real log form unmocked.
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string): Promise<T> => {
      if (cmd === 'get_snapshot') return { ...snap, link: { tier: 'FT8' }, stations: [], conversations: [], activePeer: null } as T
      if (cmd === 'get_settings') return { ...defaultSettings, fdActive: false } as T
      if (/^(get_band_plan|get_licensed_band_plan|get_need_alerts|get_scp_calls|get_voice_messages)$/.test(cmd)) return [] as T
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
  ['Phone', () => <PhoneCockpit snap={snap} theme="dark" spots={[]} onWorkSpot={() => {}} />],
  ['CW', () => <CwCockpit snap={snap} theme="dark" spots={[]} onWorkSpot={() => {}} />],
  ['RTTY', () => <RttyCockpit snap={snap} onSetTxEnabled={() => {}} />],
  ['PSK', () => <PskCockpit snap={snap} onSetTxEnabled={() => {}} />],
  ['JS8', () => <Js8Cockpit snap={snap} onSetTxEnabled={() => {}} />],
]

describe("the commit block sticks to the bottom of the cockpit's log pane", () => {
  it.each(COCKPITS)('%s: the "logs to" line and Log/Spot stick together at the bottom, opaque', async (_name, mount) => {
    render(mount())
    await settle()
    const log = document.querySelector('[data-pane="log"] .le-log-btn')
    expect(log, 'the cockpit drew no Log button in its log pane').not.toBeNull()
    // ONE block: the line that says what will be written and the row that writes it never part.
    const block = log!.closest('.le-commit')
    expect(block, 'Log is not inside the commit block').not.toBeNull()
    expect(block!.querySelector(':scope > .le-hint'), 'the "logs to … as" line left the block').not.toBeNull()
    expect(block!.querySelector(':scope > .le-actions'), 'the Log/Spot row left the block').not.toBeNull()

    expect(winner(block!, 'position'), 'the block does not stick').toBe('sticky')
    // Flush with the bottom edge: the pane body's padding cancelled, or the rows scrolled under the block
    // show through the gap beneath it (measured in Chrome: 8 px of them).
    expect(winner(block!, 'bottom'), 'the block does not stick flush to the BOTTOM edge').toBe('calc(-1 * var(--space-2))')
    expect(winner(block!.closest('.pane-body')!, 'padding'), 'the pane padding the inset cancels moved').toBe('var(--space-2)')
    // Bottom only. A top edge would park the block over the recall card that follows it.
    expect(winner(block!, 'top'), 'a top edge would ride the block over the recall card').toBeNull()
    // The theme's own opaque page colour (redefined by every theme and by Night), never a mix with
    // transparent: the fields scrolled under the block must not read through it.
    expect(winner(block!, 'background'), 'the block is not opaque').toBe('var(--bg)')
    // Stacked over the in-flow fields it covers, and nothing else declares a stacking order on it.
    expect(winner(block!, 'z-index')).toBe('1')
    // The pane's scroller keeps a focused field clear of the block: without scroll padding, Tab into a
    // field below the fold parks it at the bottom edge, under Log.
    const body = block!.closest('[data-pane="log"] > .pane-body')
    expect(body, 'the block is not inside the log pane body').not.toBeNull()
    expect(winner(body!, 'scroll-padding-bottom'), 'a field the keyboard lands on can hide under the block').toBe('6em')

    // What bounds the block: its parent is the form itself, so it can never ride over Recent or
    // anything else that follows the form in the pane; and the recall card comes AFTER it, so a
    // bottom-sticky block (which only moves up) can never cover the card.
    expect(block!.parentElement?.classList.contains('log-entry')).toBe(true)
    // A call in the box draws the card (it renders nothing for an empty one).
    fireEvent.change(document.querySelector('[data-pane="log"] .le-call')!, { target: { value: 'W1AW' } })
    await settle()
    const recall = document.querySelector('[data-pane="log"] .recall-card')
    expect(recall, 'no recall card to check the order against').not.toBeNull()
    expect(block!.compareDocumentPosition(recall!) & Node.DOCUMENT_POSITION_FOLLOWING, 'the recall card is not below the block').toBeTruthy()
  })

  it('the control: the same form outside a cockpit log pane (Satellites) stays in flow', async () => {
    render(
      <div className="sats-log">
        <LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="satellite" />
      </div>,
    )
    await settle()
    const block = document.querySelector('.le-commit')
    expect(block, 'the form drew no commit block').not.toBeNull()
    expect(block!.querySelector('.le-log-btn')).not.toBeNull()
    expect(winner(block!, 'position')).toBeNull()
  })
})
