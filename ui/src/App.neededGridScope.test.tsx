// @vitest-environment jsdom
//
// THE DOCKED NEEDED BOARD UNDER THE SAME ROWS AND SETTINGS AS THE TORN-OFF ONE (CONTROL). It has kept the
// New grid band scope since `boardNeeds` (features/needs.ts): a 20 m row whose only reason is its grid is
// not listed while Settings ▸ Spots & Alerts holds New grid at VHF+. DetachedPanel.neededGridScope.test.tsx
// holds the torn-off board and the dashboard window's box to this same list, so the windows agree by value.
// Mounts the REAL App on the Needed view.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, waitFor } from '@testing-library/react'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  const { ROWS, REPORT_SETTINGS } = await import('./neededGridScope.testkit')
  return {
    ...auto,
    ...appApiAnswers(),
    getNeedAlerts: vi.fn(async () => ROWS),
    getSettings: vi.fn(async () => REPORT_SETTINGS),
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
import { REPORT_BOARD, boardRows } from './neededGridScope.testkit'

// THE BUDGET: App.neededPark.test.tsx's, for the same mount (15 s; a test that hangs still fails).
vi.setConfig({ testTimeout: 15_000 })

beforeEach(() => {
  localStorage.clear()
  // The board stays docked: the auto-pop would move it into its own window.
  localStorage.setItem('nexus.needed.autopop', 'off')
  // The CW cockpit switched off, as in the torn-off board's test: the board's own mode chips decide.
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: false, phone: true } }))
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) => ({
    matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

describe('the docked Needed board keeps the New grid band scope (control)', () => {
  it('All lit: no 20 m row for its grid alone, and every other reason listed', async () => {
    window.location.hash = '#needed'
    render(<App />)
    await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
    await waitFor(() => {
      // The Needed view on screen: a hidden keep-alive cockpit (APRS) wears the view's classes too.
      const board = [...document.querySelectorAll('main.needed-panel')].find((e) => !e.closest('[hidden]'))
      expect(board, 'the Needed view never mounted').toBeTruthy()
      expect(boardRows(board!)).toEqual([...REPORT_BOARD].sort())
    })
    expect(document.querySelector('.view-crash')).toBeNull()
  })
})
