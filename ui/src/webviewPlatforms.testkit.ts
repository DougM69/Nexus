// THE WEBVIEWS NEXUS RUNS IN, as the theme's default tells them apart (useTheme.ts isWindowsDesktop and
// index.html's preseed): each one's user agent, and whether Tauri's bridge is in the page. Tauri puts
// `__TAURI_INTERNALS__` in every desktop-app webview before the page's first script; a browser on the
// hosted Remote page has none.
//
// Used by the theme suites: `onPlatform('mac')` before a case, `offPlatform()` in afterEach (it puts
// jsdom's own user agent back and removes the bridge).
import { vi } from 'vitest'

export const PLATFORMS = {
  // WebView2's default user agent is Microsoft Edge's (Microsoft Learn, CoreWebView2Settings.UserAgent:
  // "The default value is the default User Agent of the Edge browser"), whose platform part says
  // "Windows NT 10.0" on Windows 10 and 11 alike (Microsoft Learn, "Detecting Microsoft Edge from your
  // website"). Nexus sets no user agent of its own (tauri.conf.json, src-tauri).
  windows: {
    ua: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0',
    desktop: true,
  },
  // WebKitGTK, measured in the real Nexus Linux binary (Ubuntu's WebKitGTK 2.52.6, 2026-10-13); it has no
  // navigator.userAgentData.
  linux: {
    ua: 'Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15',
    desktop: true,
  },
  // The Raspberry Pi runs the same WebKitGTK, whose platform part is "X11; " and uname's system and
  // machine (WebCore UserAgentGLib.cpp): aarch64 on 64-bit Raspberry Pi OS. Not measured on a Pi.
  pi: {
    ua: 'Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15',
    desktop: true,
  },
  // WKWebView with no application name (WebCore UserAgentMac.mm): the macOS version is always 10_15_7.
  mac: {
    ua: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)',
    desktop: true,
  },
  // The hosted Remote page in Chrome on Windows: the same "Windows NT", and no bridge.
  windowsBrowser: {
    ua: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
    desktop: false,
  },
  // The hosted Remote page in Safari on an iPhone.
  iphoneBrowser: {
    ua: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Mobile/15E148 Safari/604.1',
    desktop: false,
  },
} as const

export type Platform = keyof typeof PLATFORMS
export const PLATFORM_NAMES = Object.keys(PLATFORMS) as Platform[]

let agent: { mockRestore(): void } | null = null

/** Runs what follows as `name`'s webview: its user agent, and Tauri's bridge or none. */
export function onPlatform(name: Platform): void {
  offPlatform()
  const { ua, desktop } = PLATFORMS[name]
  agent = vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(ua)
  if (desktop) window.__TAURI_INTERNALS__ = {}
}

/** Back to jsdom's own webview: its user agent (no "Windows NT"), and no bridge. */
export function offPlatform(): void {
  agent?.mockRestore()
  agent = null
  delete window.__TAURI_INTERNALS__
}
