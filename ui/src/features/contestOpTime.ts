/**
 * ⭐ **Operating time** — how long a contest station was on, read off its log's timestamps by the
 * sponsors' own rule: a break of `minOffSec` (30 minutes) or more between two consecutive contacts
 * is OFF time, and the rest of the span from the first contact to the last is ON time. Sweepstakes,
 * NAQP and most QSO parties state the 30 minutes; a contest whose definition carries its own minimum
 * passes it. No shipped definition carries one today (their notes list off-time limits as not
 * modelled), so every contest uses 30.
 *
 * Every row with a time counts, a dupe and a contact in a banned mode included: the station was on
 * the air for them. A removed contact is not in the log and counts for nothing.
 */
export const OFF_TIME_MIN_SEC = 30 * 60

export interface OperatingTime {
  onSec: number
  offSec: number
  /** How many breaks made the off time. */
  breaks: number
}

/** `null` while the log holds no contact with a time. */
export function operatingTime(
  log: readonly { whenUnix?: number }[],
  minOffSec: number = OFF_TIME_MIN_SEC,
): OperatingTime | null {
  const times = log
    .map((q) => q.whenUnix ?? 0)
    .filter((t) => t > 0)
    .sort((a, b) => a - b)
  if (times.length === 0) return null
  let onSec = 0
  let offSec = 0
  let breaks = 0
  for (let i = 1; i < times.length; i++) {
    const gap = times[i] - times[i - 1]
    if (gap >= minOffSec) {
      offSec += gap
      breaks++
    } else {
      onSec += gap
    }
  }
  return { onSec, offSec, breaks }
}

/** `H:MM`, whole minutes, hours unbounded (a 48-hour contest reads `36:07`). */
export function formatOpTime(sec: number): string {
  const mins = Math.floor(sec / 60)
  return `${Math.floor(mins / 60)}:${String(mins % 60).padStart(2, '0')}`
}
