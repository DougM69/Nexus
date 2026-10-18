// THE CONTEST LOGGER WINDOW'S LAYOUT — the whole window, or the entry alone.
//
// One window, two layouts (the operator: the contest screen is cluttered, so the QSO entry pops out on
// its own as a clean window). `full` is the logger window as it has always been: the entry, the last
// contacts and the contest screen under them. `entry` is the entry alone: the call and exchange boxes,
// the dupe warning and Log, with the window's bar (A− / A+, Keep in front, the layout toggle). Both are
// the same `?panel=contestlog` window, so the entry is shared with the main window by the same claim in
// either (one Enter logs once), and the window keeps one remembered box and one pin.
//
// SHARED, not per window: the main window writes it to say which layout it is opening the window in
// ("Pop out entry" or "Logger window"), and the logger window reads it and follows the `storage` event,
// so a window already open switches when the main window asks for the other layout.
import { useEffect, useState } from 'react'
import { openPanelWindow } from '../api'

export type LoggerLayout = 'full' | 'entry'

export const LOGGER_LAYOUT_KEY = 'nexus.contestLogger.layout'

/** The layout as stored, clamped on load: `entry` only when it says so, else the whole window. */
export function loggerLayout(stored: string | null): LoggerLayout {
  return stored === 'entry' ? 'entry' : 'full'
}

function readLayout(): LoggerLayout {
  try {
    return loggerLayout(window.localStorage.getItem(LOGGER_LAYOUT_KEY))
  } catch {
    return 'full'
  }
}

export function saveLoggerLayout(layout: LoggerLayout): void {
  try {
    window.localStorage.setItem(LOGGER_LAYOUT_KEY, layout)
  } catch {
    // A blocked store: this window still switches; the next open is the whole window.
  }
}

/** Open (or focus) the contest logger window in `layout`. */
export function openContestLogger(layout: LoggerLayout): Promise<void> {
  saveLoggerLayout(layout)
  return openPanelWindow('contestlog')
}

/** The logger window's layout and its own toggle: read at mount, and followed when another window
 *  writes it. */
export function useLoggerLayout(): [LoggerLayout, (layout: LoggerLayout) => void] {
  const [layout, setLayout] = useState(readLayout)
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === LOGGER_LAYOUT_KEY) setLayout(loggerLayout(e.newValue))
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
  }, [])
  const set = (next: LoggerLayout) => {
    saveLoggerLayout(next)
    setLayout(next)
  }
  return [layout, set]
}
