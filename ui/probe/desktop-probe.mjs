// The desktop's own boxes in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop rotor`.
//
// ROTOR: line 2 of the Rotor box beside a cockpit — the call in the log entry, its bearing and distance,
// "27° off" and Point — and line 1, the antenna's bearing, in the stock dashboard rail. styles.css records
// the stock box: the four-box rail at its default split, 300 px wide, with a 139 px body at 1366×768,
// where line 2's words take two lines. The probe serves src/probe/rotor.tsx with the app's own Vite
// config, sizes the window until the Rotor box's body is 139 px, and asserts every part of both lines is
// held by each box that clips it and is the topmost thing where it stands.
//
// THE CONTROL, run every time: the same window with the rail at its 200 px floor, where styles.css says
// line 2 needs a scroll. There it must come back NOT on screen; if it does not, the check cannot fail and
// the probe says so instead of passing.
//
// exit: 0 both held · 1 a red, or a control that could not fail · 2 usage, or the page never drew line 2
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const BODY_PX = 139
const WIDTH = 1920
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

if (process.argv[2] !== 'rotor') {
  console.error('usage: node ui/probe/desktop-probe.mjs rotor')
  process.exit(2)
}

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-desktop-probe-'))
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

/** One page: the probe at `railW`, its window `height` tall. */
async function open(railW) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const size = (height) =>
    call('Emulation.setDeviceMetricsOverride', { width: WIDTH, height, deviceScaleFactor: 1, mobile: false })
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  await size(768)
  await call('Page.navigate', { url: `${origin}/probe/rotor.html?railW=${railW}` })
  return { targetId, size, evaluate }
}

/** The page's measure, once it has stopped changing: the rose and line 2 settle a frame or two after a resize. */
async function settled(page) {
  let last = null
  for (let i = 0; i < 50; i++) {
    await sleep(150)
    const now = await page.evaluate('typeof __rotorProbe === "function" ? __rotorProbe() : null').catch(() => null)
    if (now && last && JSON.stringify(now) === JSON.stringify(last)) return now
    last = now
  }
  return last
}

/** Wait for line 2 to say how far off the antenna is: the station's answers have all arrived. */
async function drawn(page) {
  for (let i = 0; i < 200; i++) {
    const m = await page.evaluate('typeof __rotorProbe === "function" ? __rotorProbe() : null').catch(() => null)
    if (m?.aim?.includes('27° off')) return m
    await sleep(100)
  }
  return null
}

/** The window height whose Rotor box body is BODY_PX, found by halving: the body grows with the window. */
async function calibrate(page) {
  let lo = 300, hi = 2000, best = null
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    await page.size(mid)
    const m = await settled(page)
    if (m?.body == null) return null
    if (best == null || Math.abs(m.body - BODY_PX) < Math.abs(best.m.body - BODY_PX)) best = { height: mid, m }
    if (Math.abs(m.body - BODY_PX) <= 0.5) break
    if (m.body < BODY_PX) lo = mid + 1
    else hi = mid - 1
  }
  return best
}

let code = 0
try {
  const stock = await open(300)
  if (!(await drawn(stock))) {
    console.error(`desktop-probe: the Rotor box never drew line 2 with "27° off": ${JSON.stringify(await settled(stock))}`)
    process.exitCode = 2
  } else {
    const fit = await calibrate(stock)
    if (!fit) throw new Error('the Rotor box has no body to measure')
    await stock.size(fit.height)
    const m = await settled(stock)
    console.log(`desktop-probe: stock rail 300 px, window ${WIDTH}×${fit.height}: ${JSON.stringify(m)}`)
    if (Math.abs(m.body - BODY_PX) > 0.5) {
      console.error(`desktop-probe: the Rotor box body came out ${m.body} px, not ${BODY_PX}: the scenario is not the stock box`)
      code = 1
    } else if (!m.visible) {
      console.error('desktop-probe: RED — a part of line 1 or line 2 is clipped or covered in the stock Rotor box (see "seen")')
      code = 1
    }
    const floor = await open(200)
    if (!(await drawn(floor))) {
      console.error('desktop-probe: CONTROL — the 200 px rail never drew line 2')
      code = 1
    } else {
      await floor.size(fit.height)
      const c = await settled(floor)
      console.log(`desktop-probe: CONTROL floor rail 200 px, same window: ${JSON.stringify(c)}`)
      if (c.visible) {
        console.error('desktop-probe: CONTROL FAILED — line 2 at the 200 px floor came back on screen, where it needs a scroll; the check cannot fail')
        code = 1
      }
    }
    if (code === 0) console.log(`desktop-probe: PASS — line 1 and line 2 (its words on ${m.lines} lines) on screen in the stock Rotor box, and the floor control clipped`)
    process.exitCode = code
  }
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
