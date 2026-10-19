// @vitest-environment jsdom
//
// "CONTEST MODE IS ON, BUT THE CONTEST IS NOT RUNNING" on the CW and Phone log line, through the real App and
// the real cockpits. With the switch on and the station out of the contest (the re-entry on opening did not
// happen: here the app opens straight onto the cockpit, which enters nothing), the log line falls back to
// the everyday form; one line now says so, with Resume, which enters the contest's S&P (never a CQ). With the
// station in the contest, or the switch off, there is no line. The line on its own: LogEntry.contestResume.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, waitFor, within } from '@testing-library/react'
import { t } from './i18n'
import { APP_SNAPSHOT, COCKPIT_MAIN } from './appCockpits.testkit'

const { stationSettings, snap, setMode } = vi.hoisted(() => ({
  stationSettings: { current: {} as Record<string, unknown> },
  snap: { current: {} as Record<string, unknown> },
  setMode: vi.fn(),
}))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return {
    ...auto,
    ...appApiAnswers(),
    getSettings: vi.fn(async () => ({ ...stationSettings.current })),
    getSnapshot: vi.fn(async () => snap.current),
    // Every other answer the kit gives as a snapshot answers with this station's, so none puts it back.
    ...Object.fromEntries(
      ['setOperatingMode', 'setArea', 'selectPeer', 'workSpot', 'setHuntTarget'].map((k) => [k, vi.fn(async () => snap.current)]),
    ),
    subscribeSnapshot: vi.fn(() => () => {}),
    setMode,
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import App from './App'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 30_000 })
const WAIT = { timeout: 15_000 }

/** Illinois QSO Party, in S&P: the station inside the contest. */
const IN_CONTEST = {
  mode: 'fieldDay',
  fieldDay: {
    running: false,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    event: 'ilqp',
    role: 'in_state',
    log: [],
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    composingText: '599 KANE',
    sentExchange: '599 KANE',
    assistanceOn: [],
  },
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: true, phone: true } }))
  localStorage.setItem('nexus.features.wizardSeen', '1')
  localStorage.setItem('tempo-onboarded', '1')
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  stationSettings.current = { ...defaultSettings, mycall: 'KD9TAW', fdActive: true, fdEvent: 'ilqp' }
  snap.current = { ...APP_SNAPSHOT }
  setMode.mockReset()
  // Resume's answer: the station now in the contest's S&P.
  setMode.mockImplementation(async (spec: string) => {
    if (spec === 'fieldday-sp') snap.current = { ...APP_SNAPSHOT, ...IN_CONTEST }
    return snap.current
  })
})
afterEach(() => {
  cleanup()
  window.location.hash = ''
})

async function openOn(view: 'cw' | 'phone') {
  window.location.hash = `#${view}`
  render(<App />)
  return waitFor(() => {
    const main = document.querySelector(COCKPIT_MAIN[view]) as HTMLElement | null
    expect(main).not.toBeNull()
    return main!
  }, WAIT)
}
const notice = (main: HTMLElement) => within(main).queryByRole('status', { name: t('logEntry.contest.notRunning.aria') })

describe('the CW and Phone log line, Contest mode on and the contest not running', () => {
  it.each(['cw', 'phone'] as const)('%s: says so, and Resume enters the contest in S&P, never a CQ', async (view) => {
    const main = await openOn(view)
    await waitFor(() => expect(notice(main)).not.toBeNull(), WAIT)
    expect(notice(main)!.textContent).toContain(t('logEntry.contest.notRunning'))
    expect(setMode).not.toHaveBeenCalled()
    fireEvent.click(within(notice(main)!).getByRole('button', { name: t('logEntry.contest.resume') }))
    await waitFor(() => expect(setMode.mock.calls.map((c) => c[0])).toEqual(['fieldday-sp']), WAIT)
    // In the contest now: the line goes, and the contest's own log line takes its place.
    await waitFor(() => expect(notice(main)).toBeNull(), WAIT)
  })

  it('CONTROL: no line while the station is in the contest', async () => {
    snap.current = { ...APP_SNAPSHOT, ...IN_CONTEST }
    const main = await openOn('cw')
    // The contest's own log line, which is what stands in the line's place.
    await waitFor(() => expect(main.querySelector('.log-entry.log-entry-fd')).not.toBeNull(), WAIT)
    await new Promise((r) => setTimeout(r, 100))
    expect(notice(main)).toBeNull()
    expect(setMode).not.toHaveBeenCalled()
  })

  it('CONTROL: no line with Contest mode off', async () => {
    stationSettings.current = { ...stationSettings.current, fdActive: false }
    const main = await openOn('cw')
    await waitFor(() => expect(main.querySelector('.log-entry')).not.toBeNull(), WAIT)
    await new Promise((r) => setTimeout(r, 100))
    expect(notice(main)).toBeNull()
  })
})
