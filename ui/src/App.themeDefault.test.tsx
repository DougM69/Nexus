// @vitest-environment jsdom
//
// THE REAL APP ON A COMPUTER THAT NEVER CHOSE A THEME (operator, 2026-10-13): in the desktop app on
// Windows Nexus follows the computer's light or dark setting, so the page paints light on a light
// computer and dark on a dark one; on Linux, the Raspberry Pi and macOS it opens Dark until a pick, as
// it always did. Opening the app stores no choice — only a pick does. A stored pick is kept whatever
// the computer says, System included, on every platform (the controls). useTheme.test.ts holds the
// hook; this mounts App, the hook's owner, so a second writer or a default set somewhere else in the
// tree would show here.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { EN } from './i18n'
import { offPlatform, onPlatform } from './webviewPlatforms.testkit'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  const { default: defaultSettings } = await import('./components/__fixtures__/defaultSettings.json')
  return {
    ...auto,
    ...appApiAnswers(),
    // What Settings reads on mount, in real shapes (App.guideUdpLink.test.tsx's list): the auto-stub's
    // `{}` would crash the page the Colours check has to reach.
    getSettings: vi.fn(async () => ({ ...defaultSettings, mycall: 'KD9TAW' })),
    getRigModels: vi.fn(async () => []),
    getAllRigModels: vi.fn(async () => []),
    getSerialPortsDetailed: vi.fn(async () => []),
    getCredentialsStatus: vi.fn(async () => []),
    getConnectionLog: vi.fn(async () => []),
    detectRigs: vi.fn(async () => []),
    getAudioDevices: vi.fn(async () => ({ input: [], output: [] })),
    getCtyStatus: vi.fn(async () => null),
    allTxtLocation: vi.fn(async () => ''),
    diagLogLocation: vi.fn(async () => ''),
    recordingsLocation: vi.fn(async () => ''),
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

// THE BUDGET: App.neededPark.test.tsx's, for the same mount (15 s; a test that hangs still fails).
vi.setConfig({ testTimeout: 15_000 })

const DARK_QUERY = '(prefers-color-scheme: dark)'

/** The computer's setting as the webview reports it; every other query answers no. */
function computerPrefers(os: 'light' | 'dark') {
  window.matchMedia = ((q: string) => ({
    matches: q === DARK_QUERY && os === 'dark',
    media: q,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
  }) as unknown as MediaQueryList) as typeof window.matchMedia
}

async function mounted() {
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  return document.documentElement.getAttribute('data-theme')
}

beforeEach(() => {
  localStorage.clear()
  window.location.hash = ''
  document.documentElement.removeAttribute('data-theme')
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete (window as { matchMedia?: unknown }).matchMedia
  offPlatform()
})

describe('the app on a computer that never chose a theme', () => {
  it('on Windows paints light on a light computer, and stores no choice', async () => {
    onPlatform('windows')
    computerPrefers('light')
    expect(await mounted()).toBe('light')
    expect(localStorage.getItem('tempo-theme')).toBeNull()
  })

  it('on Windows paints dark on a dark computer, and stores no choice', async () => {
    onPlatform('windows')
    computerPrefers('dark')
    expect(await mounted()).toBe('dark')
    expect(localStorage.getItem('tempo-theme')).toBeNull()
  })

  it.each(['linux', 'pi', 'mac'] as const)('on %s paints Dark on a light computer, and stores no choice', async (platform) => {
    onPlatform(platform)
    computerPrefers('light')
    expect(await mounted()).toBe('dark')
    expect(localStorage.getItem('tempo-theme')).toBeNull()
  })
})

describe('a stored pick is kept, whatever the computer says (controls)', () => {
  it('Dark on a light Windows computer', async () => {
    onPlatform('windows')
    computerPrefers('light')
    localStorage.setItem('tempo-theme', 'dark')
    expect(await mounted()).toBe('dark')
    expect(localStorage.getItem('tempo-theme')).toBe('dark')
  })

  it('Light on a dark Linux computer', async () => {
    onPlatform('linux')
    computerPrefers('dark')
    localStorage.setItem('tempo-theme', 'light')
    expect(await mounted()).toBe('light')
    expect(localStorage.getItem('tempo-theme')).toBe('light')
  })

  it('System on a light Mac: System is a pick on every platform', async () => {
    onPlatform('mac')
    computerPrefers('light')
    localStorage.setItem('tempo-theme', 'system')
    expect(await mounted()).toBe('light')
    expect(localStorage.getItem('tempo-theme')).toBe('system')
  })
})

describe('Settings ▸ Colours names the colours the page paints (the seam App → Settings → Colours)', () => {
  // The standard Light's accent is its own steel blue now, not cyan, so the Accent row's default says
  // "Theme's own" there; on a dark computer the same install shows the standard Cyan. On Windows, where
  // a computer that never chose follows the computer.
  beforeEach(() => {
    onPlatform('windows')
    // A station past the first-run wizard, so nothing covers Settings; jsdom cannot scroll.
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: {} }))
    localStorage.setItem('nexus.features.wizardSeen', '1')
    Element.prototype.scrollIntoView = () => {}
    Element.prototype.scrollTo = () => {}
  })
  const accentDefault = async () => {
    fireEvent.click(await screen.findByRole('tab', { name: EN['settings.tabs.appearance'] }))
    const legend = await screen.findByText(EN['settings.colours.legend'], { selector: 'legend' })
    const group = within(legend.closest('fieldset') as HTMLElement).getByRole('group', { name: EN['palette.accent.label'] })
    return within(group).getAllByRole('button')[0].textContent
  }

  it('on a light computer the Accent default is the theme’s own', async () => {
    computerPrefers('light')
    window.location.hash = '#settings'
    expect(await mounted()).toBe('light')
    expect(await accentDefault()).toBe(EN['palette.preset.themeOwn'])
  })

  it('on a dark computer it is the standard Cyan', async () => {
    computerPrefers('dark')
    window.location.hash = '#settings'
    expect(await mounted()).toBe('dark')
    expect(await accentDefault()).toBe(EN['palette.preset.cyan'])
  })
})
