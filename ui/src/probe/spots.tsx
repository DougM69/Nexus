// THE SPOTS WINDOW IN A REAL BROWSER, for `scripts/browser-probe --desktop spots` (ui/probe/spots-probe.mjs
// drives it). jsdom never lays out, so nothing else can see whether the board's head — the count, the search,
// Filter and Spot me — its filter bar and its nine columns are on screen at the window's sizes. This is the
// real `?panel=spots` window (`DetachedPanel`) on the app's stylesheets, fed a busy feed: every band chip, seven
// modes, fifteen states, six continents and forty spotting countries in the filter bar, long entity names and
// comments, a few stations worked today and a few heard only on another continent. The station's answers are
// given at the IPC bridge; a command not answered here is refused.
//
// `?panel=spots` makes the page the Spots window as Rust opens it, fitting its scale to the window by the board's
// declared natural footprint. `?theme=light` paints the light theme (dark otherwise). `?zoom=100` with NO `panel`
// pins the scale verbatim, as the main window does, so the probe can measure the board in CSS px: a pop-out caps
// a pinned scale at what its window can fit (useScale's capPinnedScale), which would hide the very overflow the
// measurement looks for.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { DetachedPanel } from '../DetachedPanel'
import { surfaceSet } from '../features/windowScope'
import { naturalFor } from '../useScale'
import settings from '../components/__fixtures__/defaultSettings.json'

const params = new URLSearchParams(location.search)
localStorage.clear()
sessionStorage.clear()
localStorage.setItem('tempo-theme', params.get('theme') === 'light' ? 'light' : 'dark')
const zoom = params.get('zoom')
if (zoom) surfaceSet('nexus-ui-scale-mode', zoom)

/** Where the voices for these spots were heard from: the spotted-from chips. */
const COUNTRIES = [
  'United States', 'Canada', 'Mexico', 'Brazil', 'Argentina', 'Chile', 'England', 'Scotland', 'Ireland',
  'Federal Republic of Germany', 'Netherlands', 'Belgium', 'France', 'Spain', 'Portugal', 'Italy', 'Switzerland',
  'Austria', 'Czech Republic', 'Slovak Republic', 'Poland', 'Hungary', 'Slovenia', 'Croatia', 'Denmark', 'Sweden',
  'Norway', 'Finland', 'Estonia', 'Lithuania', 'European Russia', 'Ukraine', 'Japan', 'Republic of Korea', 'Taiwan',
  'Asiatic Russia', 'Kazakhstan', 'Australia', 'New Zealand', 'South Africa',
]
const CONTINENTS = ['NA', 'SA', 'EU', 'AS', 'OC', 'AF']
const STATES = ['CA', 'TX', 'NY', 'FL', 'OH', 'IL', 'MI', 'WA', 'AZ', 'CO', 'GA', 'NC', 'PA', 'VA', 'MN']
/** Every band chip and mode the board knows, each where it is worked. */
const CHANNELS: [string, number, string, string | null][] = [
  ['160m', 1.822, 'CW', 'CW'], ['80m', 3.525, 'CW', 'CW'], ['80m', 3.573, 'Digital', 'FT8'], ['60m', 5.357, 'Digital', 'FT8'],
  ['40m', 7.074, 'Digital', 'FT8'], ['40m', 7.188, 'Phone', null], ['30m', 10.136, 'Digital', 'FT8'], ['30m', 10.112, 'CW', 'CW'],
  ['20m', 14.025, 'CW', 'CW'], ['20m', 14.08, 'Digital', 'FT4'], ['20m', 14.085, 'Digital', 'RTTY'], ['20m', 14.071, 'Digital', 'PSK'],
  ['20m', 14.25, 'Phone', null], ['17m', 18.1, 'Digital', 'FT8'], ['15m', 21.025, 'CW', 'CW'], ['15m', 21.3, 'Phone', null],
  ['12m', 24.915, 'Digital', 'FT8'], ['10m', 28.074, 'Digital', 'FT8'], ['10m', 28.45, 'Phone', null], ['6m', 50.313, 'Digital', 'FT8'],
  ['2m', 144.174, 'Digital', 'FT8'], ['20m', 14.105, 'Digital', null],
]
/** Stations, their entities (some of cty.dat's longest names) and, for the US ones, a state. */
const STATIONS: [string, string, boolean][] = [
  ['K1ABC', 'United States', true], ['JA1XYZ', 'Japan', false], ['DL1ABC', 'Federal Republic of Germany', false],
  ['VK2XYZ', 'Australia', false], ['KP4/W1ABC', 'Puerto Rico', false], ['9Q1ABC', 'Democratic Republic of the Congo', false],
  ['W9XYZ', 'United States', true], ['9Y4XYZ', 'Trinidad & Tobago', false], ['E71ABC', 'Bosnia-Herzegovina', false],
  ['V47XYZ', 'St. Kitts & Nevis', false], ['UA9ABC', 'Asiatic Russia', false], ['PY2XYZ', 'Brazil', false],
  ['N2ABC', 'United States', true], ['ZS6XYZ', 'South Africa', false], ['VP2V/K1XYZ', 'British Virgin Islands', false],
  ['OH2ABC', 'Finland', false],
]
const CENTROIDS: Record<string, [number, number]> = {
  'United States': [39, -98], Japan: [36, 138], 'Federal Republic of Germany': [51, 10], Australia: [-25, 134],
  'Puerto Rico': [18.2, -66.5], 'Democratic Republic of the Congo': [-3, 23], 'Trinidad & Tobago': [10.5, -61.3],
  'Bosnia-Herzegovina': [44, 18], 'St. Kitts & Nevis': [17.3, -62.7], 'Asiatic Russia': [60, 100], Brazil: [-10, -52],
  'South Africa': [-29, 24], 'British Virgin Islands': [18.4, -64.6], Finland: [64, 26],
}
const COMMENTS = [
  '24 dB 26 WPM CQ', 'CQ DX UP 2, QSX 14.027 — big signal into EU all morning', '-12 dB 1567 Hz', 'POTA K-1234 Shenandoah NP',
  '', 'TNX QSO 73', 'CQ TEST', 'Loud on the long path, QSB',
]

const SPOTS = Array.from({ length: 64 }, (_, i) => {
  const [band, base, mode, submode] = CHANNELS[i % CHANNELS.length]
  const [call, entity, us] = STATIONS[i % STATIONS.length]
  const worked = i % 9 === 4
  return {
    call,
    entity,
    zone: 5,
    state: us ? STATES[i % STATES.length] : null,
    band,
    freqMhz: base + (i % 5) * 0.001,
    mode,
    submode,
    spotter: `${['W3LPL', 'DK9IP', 'VE7CC', 'K9LC', 'OH6BG', 'JH7CSU'][i % 6]}-#`,
    corroborators: [],
    ageSecs: (i * 17) % 1200,
    comment: COMMENTS[i % COMMENTS.length],
    licensed: i % 16 !== 7,
    spotterLocal: i % 7 !== 3,
    spotterConts: [CONTINENTS[i % CONTINENTS.length], CONTINENTS[(i + 2) % CONTINENTS.length]],
    spotterEntities: [COUNTRIES[i % COUNTRIES.length], COUNTRIES[(i + 13) % COUNTRIES.length]],
    ...(worked ? { workedAgoSecs: 900, workedTodayUtc: true } : {}),
  }
})
const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: { dialMhz: 14.025, band: '20m', catOk: true, sideband: 'CW', operatingMode: 'cw', transmitting: false, txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  logTick: 1,
}

const answers: Record<string, () => unknown> = {
  get_snapshot: () => snapshot,
  get_settings: () => settings,
  get_all_spots: () => SPOTS,
  get_need_alerts: () => [],
  get_band_plan: () => [],
  get_propagation: () => null,
  dxcc_entity_locations: () => Object.entries(CENTROIDS).map(([name, [lat, lon]]) => [name, lat, lon]),
}
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd]() as T)
    return Promise.reject(new Error(`${cmd} is not answered in the Spots window probe`))
  },
}
/** The box the window declares as its natural footprint (useScale's PANEL_NATURAL), for the probe to check. */
;(window as unknown as { __spotsNatural: unknown }).__spotsNatural = naturalFor('spots')

createRoot(document.getElementById('root')!).render(<DetachedPanel panel="spots" />)
