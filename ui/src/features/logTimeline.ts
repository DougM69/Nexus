// The Statistics view's timeline (components/StatsTimeline.tsx): which modes get a line, and what
// each bucket is called. The counting is the engine's (`get_log_timeline`, tempo-core's
// `TimelineCounter`), done where the log is; this reads its answer. No React, no IO.

import type { LogTimeline, TimelineRange } from '../types'
import { compareTallies } from './logStats'
import { fmtUtc } from './logQuery'
import { utcDate } from './utcLog'

/** The most lines the chart draws. A line is told apart by its dash pattern and marker shape as
 * well as its colour, and six is as many of those as stay apart on the narrowest card. Past six
 * modes, the five most-worked in the range keep a line each and the rest share one "Other" line;
 * a sixth mode alone keeps its own, as an "Other" holding one mode would only hide its name. */
export const TIMELINE_LINES = 6

/** One line of the chart. */
export interface TimelineLine {
  /** The mode, as the "By mode" card names it; null for the line summing the modes past the top
   * five. */
  mode: string | null
  /** Contacts in each bucket, oldest first. */
  counts: number[]
  /** Contacts in the whole window. */
  total: number
}

const sum = (xs: readonly number[]) => xs.reduce((s, x) => s + x, 0)

/** The lines to draw, most-worked first (ties by name, as the "By mode" card orders its bars):
 * every mode while there are at most TIMELINE_LINES, else the TIMELINE_LINES - 1 most-worked and
 * one line summing the rest. */
export function timelineLines(t: LogTimeline): TimelineLine[] {
  const ranked: TimelineLine[] = t.modes
    .map((m) => ({ mode: m.label, counts: m.counts, total: sum(m.counts) }))
    .sort((a, b) => compareTallies({ label: a.mode, count: a.total }, { label: b.mode, count: b.total }))
  if (ranked.length <= TIMELINE_LINES) return ranked
  const rest = ranked.slice(TIMELINE_LINES - 1)
  const counts = rest[0].counts.map((_, i) => sum(rest.map((m) => m.counts[i])))
  return [...ranked.slice(0, TIMELINE_LINES - 1), { mode: null, counts, total: sum(counts) }]
}

/** A bucket's name, as the log writes its times (UTC): an hour as the Logbook writes a time
 * (`2026-10-14 14:00Z`), a day as its date (`2026-10-14`), a month as its year and month
 * (`2026-10`). `start` is the bucket's first instant. */
export function bucketName(range: TimelineRange, start: number): string {
  if (range === 'pastDay') return fmtUtc(start)
  return range === 'past30Days' ? utcDate(start) : utcDate(start).slice(0, 7)
}

/** A bucket's shorter name on the axis: the hour (`14:00Z`), the day (`10-14`), the month. */
export function tickName(range: TimelineRange, start: number): string {
  if (range === 'pastDay') return fmtUtc(start).slice(-6)
  return range === 'past30Days' ? utcDate(start).slice(5) : utcDate(start).slice(0, 7)
}

/** How many buckets apart the axis names them, counted back from the current one, which is
 * always named. */
export const TICK_EVERY: Record<TimelineRange, number> = { pastDay: 6, past30Days: 7, pastYear: 3 }
