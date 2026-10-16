// THE DESKTOP ROTOR BOX IN A REAL BROWSER, for `scripts/browser-probe --desktop rotor` (ui/probe/desktop-probe.mjs
// drives it). jsdom never lays out, and the hosted Remote page has no line 2, so nothing else can see whether
// the Rotor box's line 2 is on screen. This is the real RotorPane in the real dashboard rail, on the app's
// stylesheets, beside a cockpit whose log entry holds a call. api.ts is the real one too: the station's
// answers are given here, at the IPC bridge it calls, and a command not answered here is refused.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { useViewport } from '../useViewport'
import { OwnedDashRail } from '../components/DashRail.testkit'
import { saveRailWidth } from '../features/dashRail'
import { surfaceSet } from '../features/windowScope'
import settings from '../components/__fixtures__/defaultSettings.json'

/** The call in the log entry, the antenna's azimuth and the bearing the station resolves for the call:
 *  200° against 227.4°, so line 2 says "27° off". */
const STATION = { call: 'EC1DD', az: 200, bearing: 227.4, km: 1530.6 }

document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
saveRailWidth(Number(new URLSearchParams(location.search).get('railW') ?? 300))
// The stock four boxes, with the Rotor where the clock stands.
surfaceSet(
  'nexus.dashrail.config',
  JSON.stringify({ slots: { rail1: 'rotor', rail2: 'bandTiles', rail3: 'spacewx', rail4: 'getout' } }),
)

const answers: Record<string, () => unknown> = {
  get_settings: () => ({ ...settings, mygrid: 'JO21EV', rotatorModel: 603 }),
  read_rotator_state: () => ({ azDeg: STATION.az, reading: 'position', elDeg: null, elRange: null }),
  read_rotator: () => STATION.az,
  rotator_bearing_to_call: () => ({
    pointed: { bearing: STATION.bearing, to: 'grid', grid: 'IN52TK', country: null },
    km: STATION.km,
  }),
  get_declination: () => 0,
  sat_track_status: () => null,
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd]() as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the rotor probe`))
  },
}

function Probe() {
  useViewport(1)
  return (
    <div className="app">
      <div style={{ display: 'flex', flex: '1 1 auto', minHeight: 0 }}>
        <div style={{ flex: '1 1 auto' }} />
        <OwnedDashRail
          section="operate"
          myGrid="JO21EV"
          theme="dark"
          stations={[]}
          prop={null}
          needByCall={new Map()}
          entryCall={STATION.call}
          onHide={() => {}}
        />
      </div>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)

/** `outer` holds `inner`, to half a pixel. */
const holds = (outer: DOMRect, inner: DOMRect) =>
  inner.left >= outer.left - 0.5 &&
  inner.right <= outer.right + 0.5 &&
  inner.top >= outer.top - 0.5 &&
  inner.bottom <= outer.bottom + 0.5

/** Each line of text `el` lays out (a wrapped one has several) and its own box, and whether each is on
 *  screen: held by every ancestor that clips its overflow and by the window, and the topmost thing at its
 *  centre. */
function onScreen(el: Element) {
  const clips = [new DOMRect(0, 0, innerWidth, innerHeight)]
  for (let a = el.parentElement; a; a = a.parentElement) {
    const s = getComputedStyle(a)
    if (s.overflowX === 'visible' && s.overflowY === 'visible') continue
    // The box inside its borders and scrollbars: what scrolls out of that is out of view.
    const r = a.getBoundingClientRect()
    clips.push(new DOMRect(r.left + a.clientLeft, r.top + a.clientTop, a.clientWidth, a.clientHeight))
  }
  return [...el.getClientRects(), ...textLines(el)]
    .filter((b) => b.width > 0 && b.height > 0)
    .map((b) => {
      const hit = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2)
      return {
        top: Math.round(b.top),
        bottom: Math.round(b.bottom),
        held: clips.every((c) => holds(c, b)),
        onTop: hit != null && el.contains(hit),
      }
    })
}

/** The box of each line of text inside `el`. */
function textLines(el: Element): DOMRect[] {
  const range = document.createRange()
  range.selectNodeContents(el)
  return [...range.getClientRects()]
}

/** The rotor box as laid out now: its body's height, line 2's text and how many lines it takes, and each
 *  part of lines 1 and 2 on screen or not. */
function measure() {
  const pane = document.querySelector('.dash-rail .rotor-pane')
  const body = pane?.closest('.pane-body')
  const parts = {
    'line 1': pane?.querySelector('.rotor-az'),
    'line 2': pane?.querySelector('.rotor-aim-to'),
    off: pane?.querySelector('.rotor-aim-off'),
    Point: pane?.querySelector('.rotor-aim-point'),
  }
  const seen = Object.fromEntries(Object.entries(parts).map(([name, el]) => [name, el ? onScreen(el) : null]))
  // How many lines line 2's words take (two in the stock box, styles.css says), told apart by their tops.
  const tops = parts['line 2'] ? textLines(parts['line 2']).map((b) => b.top) : []
  const lines = tops.filter((top, i) => tops.findIndex((t) => Math.abs(t - top) < 4) === i).length
  return {
    body: body ? Math.round(body.getBoundingClientRect().height * 100) / 100 : null,
    viewport: document.documentElement.getAttribute('data-viewport'),
    aim: pane?.querySelector('.rotor-aim')?.textContent ?? null,
    lines,
    seen,
    visible: Object.values(seen).every((boxes) => boxes != null && boxes.length > 0 && boxes.every((b) => b.held && b.onTop)),
    refused: [...refused].sort(),
  }
}

Object.assign(window, { __rotorProbe: measure })
