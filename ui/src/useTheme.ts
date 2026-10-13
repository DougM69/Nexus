import { useCallback, useEffect, useState } from 'react'

/** The theme the page PAINTS: what `data-theme` carries and what every component renders
 *  against. Always light or dark, so no selector and no theme-keyed cache ever sees 'system'. */
export type Theme = 'light' | 'dark'
/** What the operator PICKED, and what `tempo-theme` stores: a theme, or 'system' — follow the
 *  computer's own light/dark setting. */
export type ThemeChoice = Theme | 'system'

const STORAGE_KEY = 'tempo-theme'
const DARK_QUERY = '(prefers-color-scheme: dark)'

/** The computer's light/dark setting as a theme; Dark when the platform cannot say (the page's
 *  own default). index.html's preseed carries the first-paint copy of this. */
export function systemTheme(): Theme {
  if (typeof window.matchMedia !== 'function') return 'dark'
  return window.matchMedia(DARK_QUERY).matches ? 'dark' : 'light'
}

/** The computer's light/dark setting, followed live while `active` — the listener exists only
 *  while something is actually following it. */
export function useSystemTheme(active: boolean): Theme {
  const [os, setOs] = useState<Theme>(systemTheme)
  useEffect(() => {
    if (!active || typeof window.matchMedia !== 'function') return
    const mq = window.matchMedia(DARK_QUERY)
    const follow = () => setOs(mq.matches ? 'dark' : 'light')
    follow()
    mq.addEventListener('change', follow)
    return () => mq.removeEventListener('change', follow)
  }, [active])
  return os
}

/** The desktop app on Windows: the one place a computer that never chose a theme follows its own
 *  light/dark setting (operator, 2026-10-13). Linux, the Raspberry Pi and macOS open Dark until a
 *  pick, as they always did, and System is a pick on every platform. Windows is what the webview's
 *  user agent says: WebView2's is Edge's, "(Windows NT 10.0; Win64; x64)" on Windows 10 and 11 alike,
 *  where WebKitGTK says "(X11; …Linux …)" and WKWebView "(Macintosh; Intel Mac OS X 10_15_7)". Not
 *  navigator.userAgentData: WebKitGTK and WKWebView have none, and Chromium offers it only in a secure
 *  context. The desktop app is Tauri's bridge, which Tauri puts in each of its webviews before the
 *  page's first script; a browser on the hosted Remote page has none, so the page keeps Dark there on
 *  Windows too. index.html's preseed carries the first-paint copy (index-preseed.test.ts holds the two
 *  together, platform by platform). */
function isWindowsDesktop(): boolean {
  return navigator.userAgent.includes('Windows NT') && window.__TAURI_INTERNALS__ !== undefined
}

function readInitial(): ThemeChoice {
  const saved = localStorage.getItem(STORAGE_KEY)
  if (saved === 'light' || saved === 'dark' || saved === 'system') return saved
  // the amber theme was removed — migrate any saved value to dark so it doesn't recur
  if (saved === 'amber') {
    localStorage.setItem(STORAGE_KEY, 'dark')
    return 'dark'
  }
  // Nothing chosen follows the computer in the desktop app on Windows (operator, 2026-10-13,
  // reversing 2026-09-26's "System is a choice, not the default" there); everywhere else it is the
  // page's own Dark, as before. A saved Dark stays Dark: this hook stored its old Dark default on
  // every launch, so a saved one cannot be told from a pick, and only a pick writes one now.
  return isWindowsDesktop() ? 'system' : 'dark'
}

/** [the painted theme, set the choice, the choice itself (for the Settings chips)]. */
export function useTheme(): [Theme, (t: ThemeChoice) => void, ThemeChoice] {
  const [choice, setChoice] = useState<ThemeChoice>(readInitial)
  const os = useSystemTheme(choice === 'system')
  const theme: Theme = choice === 'system' ? os : choice

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
  }, [theme])

  // The CHOICE is stored, never the resolved theme — or System would become a fixed theme the
  // next time the app starts — and only by a pick, never by the mount, so "never chose" stays
  // readable: no key.
  const setTheme = useCallback((t: ThemeChoice) => {
    localStorage.setItem(STORAGE_KEY, t)
    setChoice(t)
  }, [])
  return [theme, setTheme, choice]
}
