// The contest logger window in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop contestlog`.
//
// The window drags down to 420×320 (`panel_min_inner("contestlog")` in src-tauri) so a logger can drop it
// in a corner beside another program and keep it in front. At that floor, at 640×480 and at 2560×1440 the
// probe serves src/probe/contestlog.tsx (the real `?panel=contestlog` window, a dupe in its entry) with the
// app's own Vite config and asserts every part the person logging needs — the Call box, the exchange
// boxes, Log, the dupe warning, the newest contact and Keep in front — is held by each box that clips it,
// inside the window, and the topmost thing where it stands.
//
// THE CONTROL, run every time: the same window 100 px shorter than the floor, where the newest contact
// falls past the bottom of the window. It must come back NOT on screen; if it does not, the check cannot
// fail and the probe says so instead of passing.
//
// exit: 0 every size held · 1 a red, or a control that could not fail · 2 the page never drew the entry
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const FLOOR = { w: 420, h: 320 }
const SIZES = [FLOOR, { w: 640, h: 480 }, { w: 2560, h: 1440 }]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** Each part, and whether it is on screen: inside the window, held by every ancestor that clips its
 *  overflow, and the topmost thing at its centre. Plus the zoom the window fitted itself to. */
const MEASURE = `(() => {
  const holds = (o, i) => i.left >= o.left - 0.5 && i.right <= o.right + 0.5 && i.top >= o.top - 0.5 && i.bottom <= o.bottom + 0.5
  const win = { left: 0, top: 0, right: innerWidth, bottom: innerHeight }
  const onScreen = (el) => {
    if (!el) return false
    const r = el.getBoundingClientRect()
    if (r.width < 1 || r.height < 1 || !holds(win, r)) return false
    for (let a = el.parentElement; a; a = a.parentElement) {
      const s = getComputedStyle(a)
      if (/(hidden|auto|scroll|clip)/.test(s.overflowX + s.overflowY) && !holds(a.getBoundingClientRect(), r)) return false
    }
    const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)
    return top != null && (top === el || el.contains(top))
  }
  const entry = document.querySelector('.contest-logger-scaled')
  if (!entry) return null
  const boxes = [...entry.querySelectorAll('input.le-fd-input')]
  const dupe = [...entry.querySelectorAll('[role=alert]')].find((a) => /dupe/i.test(a.textContent ?? ''))
  const parts = {
    call: entry.querySelector('input.le-fd-input-call'),
    exchange: boxes.filter((b) => !b.classList.contains('le-fd-input-call')),
    log: entry.querySelector('.le-fd-log-btn'),
    dupe,
    // The newest contact's cells — its time, call, exchange, band and mode — not the row's box: the row's
    // own bottom padding may sit under the scroller's edge while every word of it is in view.
    newest: [...(document.querySelector('.contest-logger-recent .fd-log-row')?.children ?? [])],
    pin: document.querySelector('.contest-logger-pin'),
  }
  const seen = {}
  for (const [k, v] of Object.entries(parts)) seen[k] = Array.isArray(v) ? v.length > 0 && v.every(onScreen) : onScreen(v)
  return { zoom: getComputedStyle(document.querySelector('.app')).zoom, viewport: document.documentElement.dataset.viewport, exchange: parts.exchange.length, seen }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-contestlog-probe-'))
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

/** One window of `w`×`h`, the logger in it, measured once its parts have stopped moving. */
async function measure(w, h) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/contestlog.html?panel=contestlog` })
  let last = null
  for (let i = 0; i < 100; i++) {
    await sleep(150)
    const r = await call('Runtime.evaluate', { expression: MEASURE, returnByValue: true })
    const now = r.exceptionDetails ? null : r.result.value
    // Drawn (the entry and its boxes are there) and still for one beat: the snapshot and the fit have landed.
    if (now && now.exchange >= 2 && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

let code = 0
try {
  for (const { w, h } of SIZES) {
    const m = await measure(w, h)
    console.log(`contestlog-probe: window ${w}×${h}: ${JSON.stringify(m)}`)
    if (!m) {
      console.error(`contestlog-probe: the logger never drew its entry at ${w}×${h}`)
      code = 2
    } else if (m.exchange < 2 || !Object.values(m.seen).every(Boolean)) {
      console.error(`contestlog-probe: RED at ${w}×${h} — a part is clipped, covered or missing (see "seen")`)
      code = code || 1
    }
  }
  const c = await measure(FLOOR.w, FLOOR.h - 100)
  console.log(`contestlog-probe: CONTROL window ${FLOOR.w}×${FLOOR.h - 100}: ${JSON.stringify(c)}`)
  if (!c || c.seen.newest) {
    console.error('contestlog-probe: CONTROL FAILED — the newest contact under the floor came back on screen; the check cannot fail')
    code = code || 1
  }
  if (code === 0) console.log(`contestlog-probe: PASS — the entry, the dupe warning, the newest contact and Keep in front on screen at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')}, and the control below the floor clipped`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
