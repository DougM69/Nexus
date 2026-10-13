// @vitest-environment jsdom
//
// THE THEME — Light / Dark / System. A computer that never chose FOLLOWS THE COMPUTER in the desktop
// app on Windows (the operator's pick of 2026-10-13, reversing 2026-09-26's "System is a choice, not
// the default", narrowed the same day to Windows): the default there is System for a new install and
// for any webview that has no stored choice. On Linux, the Raspberry Pi and macOS, and on the hosted
// Remote page in any browser, it opens Dark until a pick, as it always did; System is a pick on every
// platform. A stored choice is kept exactly — Dark and Light alike — and only a PICK writes one: Nexus
// saved its old Dark default on every launch since the first release, which is why a saved Dark cannot
// be told from a picked one, and storing nothing until the operator picks is what keeps "never chose"
// readable from now on.
//
// The shape that keeps every [data-theme] selector and every theme-keyed canvas cache as it is:
// the operator's CHOICE ('system' included) is what is stored in `tempo-theme`, and the theme
// the page PAINTS — what `data-theme` carries and what the hook returns — is always the resolved
// 'light' or 'dark'. index-preseed.test.ts holds the first-paint copy of the resolution.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useTheme } from './useTheme'
import { offPlatform, onPlatform, PLATFORM_NAMES } from './webviewPlatforms.testkit'

const QUERY = '(prefers-color-scheme: dark)'
const attr = () => document.documentElement.getAttribute('data-theme')

/** A controllable prefers-color-scheme (jsdom has no matchMedia). */
function osPrefers(initial: 'dark' | 'light') {
  let dark = initial === 'dark'
  const listeners = new Set<() => void>()
  const mql = {
    get matches() {
      return dark
    },
    media: QUERY,
    addEventListener: (_t: string, f: () => void) => listeners.add(f),
    removeEventListener: (_t: string, f: () => void) => listeners.delete(f),
  }
  window.matchMedia = ((q: string) => {
    if (q !== QUERY) throw new Error(`unexpected media query ${q}`)
    return mql
  }) as unknown as typeof window.matchMedia
  return {
    flip(to: 'dark' | 'light') {
      dark = to === 'dark'
      act(() => listeners.forEach((f) => f()))
    },
    listeners,
  }
}

afterEach(() => {
  localStorage.clear()
  document.documentElement.removeAttribute('data-theme')
  delete (window as { matchMedia?: unknown }).matchMedia
  offPlatform()
})

/** Every webview but the Windows desktop app's: Linux, the Pi and macOS, and the hosted page's browsers. */
const OPENS_DARK = PLATFORM_NAMES.filter((p) => p !== 'windows')

describe('useTheme: a computer that never chose a theme', () => {
  it('the desktop app on Windows paints what the computer prefers, light or dark, and the choice is System', () => {
    onPlatform('windows')
    for (const os of ['light', 'dark'] as const) {
      osPrefers(os)
      const { result, unmount } = renderHook(() => useTheme())
      expect(result.current[0], `OS ${os}`).toBe(os)
      expect(result.current[2], `OS ${os}`).toBe('system')
      expect(attr(), `OS ${os}`).toBe(os)
      unmount()
    }
  })

  it('Linux, the Raspberry Pi, macOS and the hosted Remote page in any browser open Dark, on a light computer too', () => {
    // Follow the computer on Windows only, and let operators elsewhere pick (System is one of the
    // picks). A browser on Windows is the hosted page, not the desktop app: its "Windows NT" alone
    // does not make it follow.
    expect(OPENS_DARK).toEqual(['linux', 'pi', 'mac', 'windowsBrowser', 'iphoneBrowser'])
    for (const platform of OPENS_DARK) {
      onPlatform(platform)
      for (const os of ['light', 'dark'] as const) {
        osPrefers(os)
        document.documentElement.removeAttribute('data-theme')
        const { result, unmount } = renderHook(() => useTheme())
        expect(result.current[0], `${platform}, ${os} computer`).toBe('dark')
        expect(result.current[2], `${platform}, ${os} computer: the choice`).toBe('dark')
        expect(attr(), `${platform}, ${os} computer`).toBe('dark')
        unmount()
      }
    }
  })

  it('opening the app stores nothing on any platform: only a pick writes the choice', () => {
    // The old hook stored its default on mount, so every install that ever ran holds a Dark it may
    // never have picked. Nothing stored is the only mark of "never chose" there is.
    osPrefers('light')
    const set = vi.spyOn(Storage.prototype, 'setItem')
    for (const platform of PLATFORM_NAMES) {
      onPlatform(platform)
      const { unmount } = renderHook(() => useTheme())
      expect(set, platform).not.toHaveBeenCalled()
      expect(localStorage.getItem('tempo-theme'), platform).toBeNull()
      unmount()
    }
    const { result } = renderHook(() => useTheme())
    act(() => result.current[1]('light'))
    expect(localStorage.getItem('tempo-theme')).toBe('light')
    set.mockRestore()
  })

  it('on Windows the default follows a change of the computer\'s setting, as a picked System does', () => {
    onPlatform('windows')
    const os = osPrefers('light')
    const { result } = renderHook(() => useTheme())
    expect(attr()).toBe('light')
    os.flip('dark')
    expect(attr(), 'the OS went dark and the page did not follow').toBe('dark')
    expect(result.current[0]).toBe('dark')
    expect(localStorage.getItem('tempo-theme')).toBeNull()
  })

  it('elsewhere the default Dark holds when the computer changes, and nothing listens to it', () => {
    onPlatform('linux')
    const os = osPrefers('dark')
    renderHook(() => useTheme())
    expect(attr()).toBe('dark')
    expect(os.listeners.size, 'a default Dark listens to the computer').toBe(0)
    os.flip('light')
    expect(attr(), 'a default Dark followed the computer going light').toBe('dark')
  })

  it('a stored value this build does not know is nothing stored: System on Windows, Dark elsewhere', () => {
    for (const [platform, theme, choice] of [['windows', 'light', 'system'], ['linux', 'dark', 'dark'], ['mac', 'dark', 'dark']] as const) {
      onPlatform(platform)
      osPrefers('light')
      localStorage.setItem('tempo-theme', 'sepia')
      const { result, unmount } = renderHook(() => useTheme())
      expect(result.current[0], platform).toBe(theme)
      expect(result.current[2], platform).toBe(choice)
      unmount()
      localStorage.clear()
    }
  })

  it('a saved Light, Dark or System is kept exactly on every platform, whatever the computer prefers (controls)', () => {
    for (const platform of PLATFORM_NAMES) {
      onPlatform(platform)
      for (const saved of ['light', 'dark', 'system'] as const) {
        // The computer disagrees with a fixed pick; System is a pick everywhere and follows it.
        const os = saved === 'light' ? 'dark' : 'light'
        const want = saved === 'system' ? os : saved
        osPrefers(os)
        localStorage.setItem('tempo-theme', saved)
        document.documentElement.removeAttribute('data-theme')
        const { result, unmount } = renderHook(() => useTheme())
        expect(result.current[0], `${platform}, saved ${saved}`).toBe(want)
        expect(result.current[2], `${platform}, saved ${saved}`).toBe(saved)
        expect(attr(), `${platform}, saved ${saved}`).toBe(want)
        expect(localStorage.getItem('tempo-theme'), `${platform}, saved ${saved}`).toBe(saved)
        unmount()
        localStorage.clear()
      }
    }
  })

  it('the removed Amber theme still migrates to Dark, on Windows too', () => {
    for (const platform of ['windows', 'linux'] as const) {
      onPlatform(platform)
      osPrefers('light')
      localStorage.setItem('tempo-theme', 'amber')
      const { result, unmount } = renderHook(() => useTheme())
      expect(result.current[0], platform).toBe('dark')
      expect(localStorage.getItem('tempo-theme'), platform).toBe('dark')
      unmount()
      localStorage.clear()
    }
  })
})

describe('useTheme: System follows the computer', () => {
  it('resolves to the OS setting at mount, and paints only light or dark', () => {
    for (const os of ['light', 'dark'] as const) {
      osPrefers(os)
      localStorage.setItem('tempo-theme', 'system')
      const { result, unmount } = renderHook(() => useTheme())
      expect(result.current[0], `OS ${os}`).toBe(os)
      expect(result.current[2]).toBe('system')
      expect(attr()).toBe(os)
      unmount()
    }
  })

  it('follows a change of the OS setting while it is on, and keeps "system" stored', () => {
    const os = osPrefers('dark')
    localStorage.setItem('tempo-theme', 'system')
    const { result } = renderHook(() => useTheme())
    expect(attr()).toBe('dark')
    os.flip('light')
    expect(attr(), 'the OS went light and the page did not follow').toBe('light')
    expect(result.current[0]).toBe('light')
    os.flip('dark')
    expect(attr()).toBe('dark')
    // The CHOICE is what is stored: writing the resolved value would turn System into a fixed
    // theme the next time the app starts.
    expect(localStorage.getItem('tempo-theme')).toBe('system')
  })

  it('picking System from Dark applies the OS setting at once', () => {
    osPrefers('light')
    localStorage.setItem('tempo-theme', 'dark')
    const { result } = renderHook(() => useTheme())
    expect(attr()).toBe('dark')
    act(() => result.current[1]('system'))
    expect(attr()).toBe('light')
    expect(localStorage.getItem('tempo-theme')).toBe('system')
  })

  it('picking Dark after System stops following the OS', () => {
    // The mirror of the live-follow case: a listener left behind would flip a pinned theme the
    // next time the OS changed.
    const os = osPrefers('light')
    localStorage.setItem('tempo-theme', 'system')
    const { result } = renderHook(() => useTheme())
    act(() => result.current[1]('dark'))
    expect(attr()).toBe('dark')
    expect(os.listeners.size, 'the OS listener outlived System').toBe(0)
    os.flip('light')
    expect(attr()).toBe('dark')
    expect(localStorage.getItem('tempo-theme')).toBe('dark')
  })

  it('a platform that cannot say resolves System to Dark, picked or by default', () => {
    // By default is the Windows desktop app's System; picked is System anywhere.
    onPlatform('windows')
    for (const stored of ['system', null]) {
      if (stored) localStorage.setItem('tempo-theme', stored)
      const { result, unmount } = renderHook(() => useTheme())
      expect(result.current[0], `stored ${stored}`).toBe('dark')
      expect(attr(), `stored ${stored}`).toBe('dark')
      unmount()
      localStorage.clear()
    }
  })
})
