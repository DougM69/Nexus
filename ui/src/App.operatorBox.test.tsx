// @vitest-environment jsdom
//
// CTRL+O IN THE REAL APP (N1MM's "Ctrl+O = Set operator call"). From a cockpit's contest log strip it
// shows the Contest screen with the caret in its OPERATOR box, the one place the operator at the key
// is set while a contest runs. It is a key in the log line: the station's operating mode is left as it
// was, so the FT8/FT4 role the Contest tab's own button enters on the way in is not touched — the
// control below shows that button does, in this same app.
import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest'
import { act, render, screen, waitFor, within, fireEvent } from '@testing-library/react'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return { ...auto, ...appApiAnswers() }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import * as api from './api'
import App from './App'
import { APP_SNAPSHOT } from './appCockpits.testkit'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import { t } from './i18n'
import type { AppSnapshot, Settings } from './types'

// The real App mounts per case; under the full suite's load that outruns vitest's default 5 s.
vi.setConfig({ testTimeout: 30_000 })
const WAIT = { timeout: 10_000 }

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true, connect: true }
/** The Illinois QSO Party running, from Cook County. */
const ILQP = {
  running: true,
  state: 'Idle',
  event: 'ilqp',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  role: 'in_state',
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
  ],
  composing: [
    { key: 'RST', raw: '599' },
    { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
  ],
}
const CONTEST = { ...APP_SNAPSHOT, fieldDay: ILQP } as unknown as AppSnapshot

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
  // Contest mode on, as the snapshot's running contest says.
  vi.mocked(api.getSettings).mockResolvedValue({ ...settingsFixture, fdActive: true, fdEvent: 'ilqp' } as unknown as Settings)
  for (const answer of [api.getSnapshot, api.setOperatingMode, api.setArea, api.selectPeer, api.setMode]) {
    vi.mocked(answer as Mock).mockImplementation(async () => CONTEST)
  }
  vi.mocked(api.subscribeSnapshot).mockImplementation(() => () => {})
  vi.mocked(api.setMode).mockClear()
})

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), WAIT)
  await act(async () => {})
}
const operatorBox = () => screen.findByLabelText(t('fieldDay.operator.aria'), undefined, WAIT)
const cwStripCall = () =>
  waitFor(() => {
    const c = document.querySelector('main.cw-cockpit .le-fd-input-call')
    expect(c, 'the CW contest strip did not render').not.toBeNull()
    return c as HTMLInputElement
  }, WAIT)

describe('Ctrl+O in the app', () => {
  it('from the CW contest strip, shows the Contest screen with the caret in the operator box, and leaves the mode alone', async () => {
    await mountOn('cw')
    const call = await cwStripCall()
    call.focus()
    let free = true
    await act(async () => {
      free = fireEvent.keyDown(call, { key: 'o', code: 'KeyO', ctrlKey: true })
    })
    expect(free, 'Ctrl+O is the strip’s: the webview’s Open never runs').toBe(false)
    const box = await operatorBox()
    await waitFor(() => expect(document.activeElement).toBe(box), WAIT)
    // With a contest running the box is at the top: on the entry's title line, beside Sending.
    expect(box.closest('.contest-screen .contest-logger-entry .le-fd-header'), 'the caret is not in the top box').not.toBeNull()
    expect(document.querySelector('main.cw-cockpit'), 'the Contest screen is the one on show').toBeNull()
    expect(vi.mocked(api.setMode), 'the FT role is left as it was').not.toHaveBeenCalled()
  })

  it('answers an ask once: the next visit by the Contest tab leaves the caret where it was', async () => {
    await mountOn('cw')
    const call = await cwStripCall()
    await act(async () => {
      fireEvent.keyDown(call, { key: 'o', code: 'KeyO', ctrlKey: true })
    })
    const box = await operatorBox()
    await waitFor(() => expect(document.activeElement).toBe(box), WAIT)
    const nav = document.querySelector('nav.mode-nav') as HTMLElement
    fireEvent.click(within(nav).getByRole('button', { name: t('nav.cw.title') }))
    await cwStripCall()
    fireEvent.click(within(nav).getByRole('button', { name: new RegExp(t('nav.fieldDay.label')) }))
    const again = await operatorBox()
    await act(async () => new Promise((r) => setTimeout(r, 50)))
    expect(document.activeElement).not.toBe(again)
  })

  it('control: the Contest tab’s own button enters the contest’s S&P, which the mock sees', async () => {
    await mountOn('cw')
    await cwStripCall()
    const nav = document.querySelector('nav.mode-nav') as HTMLElement
    fireEvent.click(within(nav).getByRole('button', { name: new RegExp(t('nav.fieldDay.label')) }))
    await act(async () => {})
    await operatorBox()
    expect(vi.mocked(api.setMode).mock.calls.map((c) => c[0])).toEqual(['fieldday-sp'])
  })
})
