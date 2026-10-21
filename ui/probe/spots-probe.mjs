// The Spots window in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop spots`.
//
// The board's nine columns floor at a fixed width, so the window's sizes are chosen from that width at the
// 65% zoom floor. The probe serves src/probe/spots.tsx (the real `?panel=spots` window, a busy feed in it) with
// the app's own Vite config, and:
//   1. MEASURES the board's natural footprint with the scale pinned at 100% (as in the main window: a pop-out
//      caps a pin at what its window fits): the narrowest window in which its head, every column heading and the
//      first rows are on screen with nothing scrolling sideways,
//      and at the natural width the window declares, the shortest one that also holds every filter chip. Both
//      must be within that declared natural (useScale's PANEL_NATURAL), or the window would fit itself to a box
//      its content does not fit.
//   2. OPENS the window at its default size and at its smallest (`panel_default_inner` / `panel_min_inner` in
//      src-tauri, read from lib.rs), in both themes, the scale fitted as the app fits it: 100% at the default,
//      the 65% floor at the smallest. Every part of the head (the count, the search, Filter and Spot me), every
//      filter chip, every column heading and the first rows must be held by each box that clips it, inside the
//      window, and the topmost thing where it stands; nothing may scroll sideways; and scrolling the list must
//      bring its last row on screen.
//   3. THE CONTROL, run every time: the smallest window 120 px narrower, at the same floor, where the columns
//      cannot fit. It must come back NOT on screen; if it does not, the check cannot fail and the probe says so.
//
// exit: 0 every size held · 1 a red, or a control that could not fail · 2 the page never drew the board
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** The window's size from `fn <name>(slug: &str)` in src-tauri/src/lib.rs: the Spots arm, else the fallback. */
async function rustSize(fnName) {
  const lib = await readFile(join(UI, '..', 'src-tauri', 'src', 'lib.rs'), 'utf8')
  const body = new RegExp(`fn ${fnName}\\(slug: &str\\) -> \\(f64, f64\\) \\{([\\s\\S]*?)\\n\\}`).exec(lib)?.[1]
  const arm = body && (/"spots"[^=]*=>\s*\(([\d.]+),\s*([\d.]+)\)/.exec(body) ?? /_\s*=>\s*\(([\d.]+),\s*([\d.]+)\)/.exec(body))
  if (!arm) throw new Error(`${fnName}: no Spots or fallback arm found in src-tauri/src/lib.rs`)
  return { w: Number(arm[1]), h: Number(arm[2]) }
}

/** The head's parts, by name. */
const HEAD = ['title', 'count', 'search', 'filter', 'spotMeText', 'spotMe']

/** Each part of the board and whether it is on screen: inside the window, held by every ancestor that clips its
 *  overflow, and the topmost thing at its centre. Plus the zoom the window fitted itself to, and how far the
 *  list and the page scroll sideways. */
const MEASURE = `(() => {
  const holds = (o, i) => i.left >= o.left - 0.5 && i.right <= o.right + 0.5 && i.top >= o.top - 0.5 && i.bottom <= o.bottom + 0.5
  const win = { left: 0, top: 0, right: innerWidth, bottom: innerHeight }
  const name = (el) => el ? [el.tagName.toLowerCase(), ...el.classList].join('.') : 'nothing'
  // Why \`el\` is not on screen, or null when it is.
  const why = (el) => {
    if (!el) return 'missing'
    const r = el.getBoundingClientRect()
    if (r.width < 1 || r.height < 1) return 'no box'
    if (!holds(win, r)) return 'outside the window'
    for (let a = el.parentElement; a; a = a.parentElement) {
      const s = getComputedStyle(a)
      if (/(hidden|auto|scroll|clip)/.test(s.overflowX + s.overflowY) && !holds(a.getBoundingClientRect(), r)) return 'clipped by ' + name(a)
    }
    const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)
    return top != null && (top === el || el.contains(top)) ? null : 'covered by ' + name(top)
  }
  const onScreen = (el) => why(el) == null
  const board = document.querySelector('main.spots-panel')
  const grid = board?.querySelector('.sp-grid')
  if (!board || !grid) return null
  const rows = [...grid.querySelectorAll('.sp-row')]
  const head = {
    title: board.querySelector('.np-head h2'),
    count: board.querySelector('.np-head .np-count'),
    search: board.querySelector('.np-search input'),
    filter: board.querySelector('.np-filter-toggle'),
    spotMeText: board.querySelector('.np-spot-comment'),
    spotMe: board.querySelector('.np-spot-btn'),
  }
  const seen = {}
  const off = {}
  for (const [k, el] of Object.entries(head)) {
    const reason = why(el)
    seen[k] = reason == null
    if (reason) off[k] = reason
  }
  const chips = [...board.querySelectorAll('.np-filters button, .np-filters select')]
  seen.chips = chips.length > 0 && chips.every(onScreen)
  seen.columns = [...grid.querySelectorAll('.np-header > *')].every(onScreen)
  seen.firstRows = rows.length >= 3 && rows.slice(0, 3).every((r) => [...r.children].every(onScreen))
  return {
    zoom: Number(getComputedStyle(document.querySelector('.app')).zoom),
    viewport: document.documentElement.dataset.viewport,
    natural: window.__spotsNatural,
    rows: rows.length,
    chips: chips.length,
    overflowX: grid.scrollWidth - grid.clientWidth,
    pageX: document.scrollingElement.scrollWidth - innerWidth,
    toasts: document.querySelectorAll('.ui-toast').length,
    seen,
    off,
  }
})()`

/** Scroll the list to its end: is the last row then on screen? The list is put back where it was. */
const REACH = `(async () => {
  const grid = document.querySelector('main.spots-panel .sp-grid')
  const rows = grid ? [...grid.querySelectorAll('.sp-row')] : []
  if (rows.length === 0) return false
  const last = rows[rows.length - 1]
  grid.scrollTop = grid.scrollHeight
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
  const r = last.getBoundingClientRect()
  const g = grid.getBoundingClientRect()
  const top = document.elementFromPoint(r.left + Math.min(r.width, g.width) / 4, r.top + r.height / 2)
  const seen = r.top >= g.top - 0.5 && r.bottom <= g.bottom + 0.5 && r.bottom <= innerHeight + 0.5 && top != null && last.contains(top)
  grid.scrollTop = 0
  return seen
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-spots-probe-'))
const server = await createServer({
  root: UI,
  configFile: join(UI, 'vite.config.ts'),
  cacheDir,
  logLevel: 'warn',
  server: { host: '127.0.0.1', port: 0, strictPort: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

/** One window of `w`×`h` with the Spots board in it; `query` is the probe page's switches (src/probe/spots.tsx). */
async function open(w, h, query) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const size = (width, height) => call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false })
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    return r.exceptionDetails ? null : r.result.value
  }
  await size(w, h)
  await call('Page.navigate', { url: `${origin}/probe/spots.html?${query}` })
  return { size, evaluate, close: () => browser.call('Target.closeTarget', { targetId }) }
}

/** The page's measure once it is drawn and has stopped changing. Drawn: the feed listed, and the first snapshot
 *  in, since Spot me shows only once there is a station to name (the snapshot poll is not leading, and a page
 *  Chrome has not brought to the front runs its timers slowly). Then still for one beat: the fit and the
 *  wrapping head land a frame or two after a resize. */
async function settled(page) {
  let last = null
  for (let i = 0; i < 100; i++) {
    await sleep(150)
    const now = await page.evaluate(MEASURE)
    const drawn = now != null && now.rows > 0 && now.off.spotMe !== 'missing'
    if (drawn && last && JSON.stringify(now) === JSON.stringify(last)) return now
    last = now
  }
  return last
}

/** The head, every column heading and the first rows on screen, and nothing scrolling sideways. */
const columnsFit = (m) => m != null && m.overflowX <= 1 && m.pageX <= 1 && m.seen.columns && m.seen.firstRows && HEAD.every((k) => m.seen[k])
/** …and every filter chip too: the whole board as it opens. */
const whole = (m) => columnsFit(m) && m.seen.chips

/** The smallest value in [lo, hi] for which `ok(await at(v))` holds, assuming it holds from there up. */
async function smallest(lo, hi, at, ok) {
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (ok(await at(mid))) hi = mid
    else lo = mid + 1
  }
  return lo
}

let code = 0
try {
  const def = await rustSize('panel_default_inner')
  const min = await rustSize('panel_min_inner')
  console.log(`spots-probe: the window opens at ${def.w}×${def.h} and drags down to ${min.w}×${min.h} (src-tauri/src/lib.rs)`)

  // 1. THE NATURAL FOOTPRINT, at 100%.
  const page = await open(1600, 1000, 'zoom=100')
  const first = await settled(page)
  if (!first || first.rows === 0) {
    console.error(`spots-probe: the Spots window never drew its board: ${JSON.stringify(first)}`)
    process.exitCode = 2
  } else {
    const natural = first.natural
    const width = await smallest(400, 1600, async (w) => (await page.size(w, 1000), settled(page)), columnsFit)
    // The height at the width the window promises at its floor: the declared one, or the measured one where
    // the declaration is narrower (then the width check below is red anyway, and this says what to declare).
    const at = Math.max(width, natural.w)
    const height = await smallest(200, 1600, async (h) => (await page.size(at, h), settled(page)), whole)
    console.log(
      `spots-probe: natural at 100%: the columns fit from ${width} px wide; at ${at} px the whole board ` +
        `(${first.chips} filter chips) fits from ${height} px tall — the window declares ${natural.w}×${natural.h}`,
    )
    if (width > natural.w || height > natural.h) {
      console.error(`spots-probe: RED — the board needs ${width}×${height} at 100%, more than the ${natural.w}×${natural.h} the window declares`)
      code = 1
    }
    await page.close()

    // 2. THE WINDOW AT ITS DEFAULT SIZE AND AT ITS SMALLEST, IN BOTH THEMES, FITTED AS THE APP FITS IT.
    for (const theme of ['dark', 'light']) {
      for (const [name, box, zoom] of [['default', def, 1], ['smallest', min, 0.65]]) {
        const win = await open(box.w, box.h, `panel=spots&theme=${theme}`)
        const m = await settled(win)
        const reach = m ? await win.evaluate(REACH) : false
        await win.close()
        console.log(`spots-probe: ${theme}, ${name} ${box.w}×${box.h}: ${JSON.stringify({ ...m, natural: undefined, reach })}`)
        if (!m || m.rows === 0) {
          console.error(`spots-probe: the Spots window never drew its board at ${box.w}×${box.h}`)
          code = 2
        } else if (!whole(m) || !reach) {
          console.error(`spots-probe: RED, ${theme} at ${box.w}×${box.h} — a part is clipped, covered, scrolls sideways or cannot be reached (see "seen", "overflowX", "reach")`)
          code = code || 1
        } else if (Math.abs(m.zoom - zoom) > 0.001) {
          console.error(`spots-probe: RED, ${theme} at ${box.w}×${box.h} — the window fitted itself to ${m.zoom * 100}%, not ${zoom * 100}%`)
          code = code || 1
        }
      }
    }

    // 3. THE CONTROL: narrower than the smallest window, at the same floor, the columns cannot fit.
    const narrow = await open(min.w - 120, min.h, 'panel=spots&theme=dark')
    const c = await settled(narrow)
    await narrow.close()
    console.log(`spots-probe: CONTROL ${min.w - 120}×${min.h}: ${JSON.stringify({ ...c, natural: undefined })}`)
    if (!c || columnsFit(c)) {
      console.error('spots-probe: CONTROL FAILED — the columns came back on screen in a window narrower than the smallest; the check cannot fail')
      code = code || 1
    }
    if (code === 0) {
      console.log(
        `spots-probe: PASS — the board fits its declared ${natural.w}×${natural.h} at 100%; the head, every filter chip, every column ` +
          `heading and the first rows on screen, nothing sideways and the last row reached, at ${def.w}×${def.h} (100%) and ` +
          `${min.w}×${min.h} (the 65% floor) in both themes; and the control below the smallest clipped`,
      )
    }
    process.exitCode = code
  }
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
