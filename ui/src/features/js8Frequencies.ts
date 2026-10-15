// JS8 frequencies, the band schedule and the decode filter: the pure halves of their editors
// (Settings ▸ Digital ▸ JS8, and the JS8 cockpit's waterfall controls). JS8Call 3.0.3 keeps
// the same three in its Settings ▸ Frequencies tab and its waterfall controls.
import type { Js8Filter, Js8Freq, Js8Hop } from '../types'

/** JS8Call's decode filter as it ships (WideGraph.cpp:237-241): on, 1500 Hz, 2000 Hz wide. */
export const JS8_FILTER_DEFAULT: Js8Filter = { enabled: true, centerHz: 1500, widthHz: 2000 }

/** The longest description a frequency or schedule row keeps (the editors' fields hold this much). */
export const JS8_DESCRIPTION_MAX = 80

/** JS8Call's passband centre for QSY as it ships (`CenterOffset`, WideGraph.cpp:216). */
export const JS8_QSY_CENTER_DEFAULT = 1500

/** The audio window the decoder searches while the filter is on: the engine's own rule
 *  (`Engine::js8_filter_window`): centre ± width/2, inside 100–4000 Hz and at least 50 Hz wide. */
export function js8FilterWindow(f: Js8Filter): { lo: number; hi: number } {
  const half = Math.floor(f.widthHz / 2)
  const lo = Math.min(Math.max(f.centerHz - half, 100), 3950)
  const hi = Math.min(Math.max(f.centerHz + half, lo + 50), 4000)
  return { lo, hi }
}

/** A typed dial, in MHz, or null when it is not one: a finite number above 0 and below 10 GHz. */
export function parseMhz(raw: string): number | null {
  const s = raw.trim().replace(',', '.')
  if (s === '') return null
  const n = Number(s)
  return Number.isFinite(n) && n > 0 && n < 10_000 ? n : null
}

/** "HH:MM" for minutes after 00:00 UTC (the value of an `<input type="time">`). */
export function minutesToHhmm(min: number): string {
  const m = Math.min(Math.max(Math.round(min), 0), 1439)
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`
}

/** Minutes after 00:00 UTC for "HH:MM", or null when it is not a time of day. */
export function hhmmToMinutes(s: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(s.trim())
  if (!m) return null
  const h = Number(m[1])
  const mm = Number(m[2])
  return h < 24 && mm < 60 ? h * 60 + mm : null
}

/** The file Export writes: the rows, under a name that says what they are. */
export function js8FrequencyFile(rows: Js8Freq[]): string {
  return `${JSON.stringify(
    { nexusJs8Frequencies: 1, frequencies: rows.map((r) => ({ mhz: r.mhz, description: r.description })) },
    null,
    2,
  )}\n`
}

/** Read a file Export wrote (or a bare list of `{ mhz, description }`): its rows, every one with a
 *  usable dial, or null when the file holds none. A row with no usable dial makes the whole file
 *  unreadable rather than dropping it quietly. */
export function parseJs8FrequencyFile(text: string): Js8Freq[] | null {
  let doc: unknown
  try {
    doc = JSON.parse(text)
  } catch {
    return null
  }
  const list = Array.isArray(doc)
    ? doc
    : doc && typeof doc === 'object' && Array.isArray((doc as { frequencies?: unknown }).frequencies)
      ? (doc as { frequencies: unknown[] }).frequencies
      : null
  if (!list || list.length === 0) return null
  const rows: Js8Freq[] = []
  for (const item of list) {
    if (!item || typeof item !== 'object') return null
    const { mhz, description } = item as { mhz?: unknown; description?: unknown }
    const dial = typeof mhz === 'number' ? parseMhz(String(mhz)) : typeof mhz === 'string' ? parseMhz(mhz) : null
    if (dial === null) return null
    rows.push({ mhz: dial, description: typeof description === 'string' ? description.trim().slice(0, JS8_DESCRIPTION_MAX) : '' })
  }
  return rows
}

/** A schedule row as a new one starts: the frequency typed, the window from now to an hour on. */
export function newHop(mhz: number, nowMs: number, description = ''): Js8Hop {
  const fromMin = Math.floor((nowMs % 86_400_000) / 60_000)
  return { mhz, fromMin, untilMin: (fromMin + 60) % 1440, description }
}
