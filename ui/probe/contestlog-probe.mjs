// The contest logger window in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop contestlog`.
//
// The window drags down to 420×320 (`panel_min_inner("contestlog")` in src-tauri) so a logger can drop it
// in a corner beside another program and keep it in front. At that floor, at 640×480 and at 2560×1440 the
// probe serves src/probe/contestlog.tsx (the real `?panel=contestlog` window, a dupe in its entry) with the
// app's own Vite config and asserts every part the person logging needs — the Call box, the exchange
// boxes, Log, the dupe warning, the newest contact and Keep in front — is held by each box that clips it,
// inside the window, and the topmost thing where it stands. Then the entry-only layout at the floor and
// large: the entry's parts on screen and nothing under it. Then both layouts again with the optional "RST
// sent" box turned on, one more box in the entry, at the floor and large.
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
    // The optional "RST sent" box, only in the runs that turn it on (\`rstsent\`).
    ...(new URLSearchParams(location.search).get('rstsent') === '1' ? { sent: entry.querySelector('.le-fd-field-sent input') } : {}),
  }
  const seen = {}
  for (const [k, v] of Object.entries(parts)) seen[k] = Array.isArray(v) ? v.length > 0 && v.every(onScreen) : onScreen(v)
  const below = document.querySelector('.contest-logger-recent') != null || document.querySelector('.contest-logger > main.layout.single') != null
  return { zoom: getComputedStyle(document.querySelector('.app')).zoom, viewport: document.documentElement.dataset.viewport, exchange: parts.exchange.length, below, seen }
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
async function measure(w, h, layout = 'full', rstsent = false) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/contestlog.html?panel=contestlog&layout=${layout}${rstsent ? '&rstsent=1' : ''}` })
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
  // THE ENTRY ALONE: the same window in its entry-only layout, at the floor and large. Every part of the
  // entry on screen, and nothing under it — no last contacts, no contest screen.
  for (const { w, h } of [FLOOR, SIZES[2]]) {
    const m = await measure(w, h, 'entry')
    console.log(`contestlog-probe: entry only, window ${w}×${h}: ${JSON.stringify(m)}`)
    const { newest, ...entryParts } = m?.seen ?? {}
    if (!m) {
      console.error(`contestlog-probe: the entry-only window never drew its entry at ${w}×${h}`)
      code = 2
    } else if (m.below || newest || m.exchange < 2 || !Object.values(entryParts).every(Boolean)) {
      console.error(`contestlog-probe: RED entry only at ${w}×${h} — a part is off screen, or the screen is still under it`)
      code = code || 1
    }
  }
  // THE "RST SENT" BOX ON: one more box in the entry. At the floor and large, whole and entry-only, every
  // part of the entry — the box among them — is still on screen, and the whole window's newest contact
  // large. At the floor the box costs the entry a third row, so the newest contact there is one scroll
  // down (the window scrolls, as it does to reach the contest screen); that is printed, not failed.
  for (const layout of ['full', 'entry']) {
    for (const { w, h } of [FLOOR, SIZES[2]]) {
      const m = await measure(w, h, layout, true)
      console.log(`contestlog-probe: RST sent box on, ${layout}, window ${w}×${h}: ${JSON.stringify(m)}`)
      const { newest, ...entryParts } = m?.seen ?? {}
      if (!m) {
        console.error(`contestlog-probe: the window never drew its entry with the RST sent box at ${w}×${h}`)
        code = 2
      } else if (m.exchange < 3 || !('sent' in entryParts) || !Object.values(entryParts).every(Boolean) || (layout === 'full' && w !== FLOOR.w && !newest)) {
        console.error(`contestlog-probe: RED with the RST sent box, ${layout} at ${w}×${h} — a part is clipped, covered or missing`)
        code = code || 1
      } else if (layout === 'full' && !newest) {
        console.log(`contestlog-probe: note — with the RST sent box on at ${w}×${h} the newest contact is below the entry, one scroll down`)
      }
    }
  }
  const c = await measure(FLOOR.w, FLOOR.h - 100)
  console.log(`contestlog-probe: CONTROL window ${FLOOR.w}×${FLOOR.h - 100}: ${JSON.stringify(c)}`)
  if (!c || c.seen.newest) {
    console.error('contestlog-probe: CONTROL FAILED — the newest contact under the floor came back on screen; the check cannot fail')
    code = code || 1
  }
  if (code === 0) console.log(`contestlog-probe: PASS — the entry, the dupe warning, the newest contact and Keep in front on screen at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')}; the entry alone at ${FLOOR.w}×${FLOOR.h} and ${SIZES[2].w}×${SIZES[2].h}; both again with the RST sent box on; and the control below the floor clipped`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
