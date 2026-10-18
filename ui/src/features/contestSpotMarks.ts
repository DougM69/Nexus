// THE CONTEST MARKS ON A SPOT — dupe and new multiplier, the way a contest band map marks them.
//
// Neither verdict is decided here. The DUPE is the strip's own (`contestDupe`, the check behind the
// DUPE badge), asked on the spot's band in the spot's mode class. The MULTIPLIERS are the scorer's
// own: the station computes them for each spot with the contest's multiplier keys
// (`FieldDayLog::spot_mults`) and they ride the row as `contestMults`. This file only joins the two
// and words them.
//
// ⚡ A busy band map is hundreds of spots against a log of thousands, on every snapshot. So the log
// is indexed by call ONCE per snapshot, each spot's dupe check reads only its own station's rows,
// and a verdict is kept per (call, band, mode class) for the life of that snapshot.
import { useMemo } from 'react'
import { t } from '../i18n'
import type { FieldDayQso, FieldDayStatus, SpotContestMult, SpotRow } from '../types'
import { contestDupe } from './contestDupe'

export interface SpotContestMark {
  /** Already worked on this band and mode under the contest's dupe rule: working it scores zero. */
  dupe: boolean
  /** The multipliers working it would add. Always empty for a dupe, which counts for nothing. */
  mults: SpotContestMult[]
}

/** The contest log's mode class (`CW` | `PH` | `DIG`) of a spot's (`CW` | `Phone` | `Digital`) —
 *  the same map the station makes for the multipliers (src-tauri `spot_contest_class`). */
export function spotContestClass(mode: string): string {
  return mode === 'CW' ? 'CW' : mode === 'Phone' ? 'PH' : 'DIG'
}

/** The marks for any spot against one snapshot of the contest log — `null` outside a contest,
 *  and for a spot that is neither a dupe nor a new multiplier. */
export type SpotMarker = (s: SpotRow) => SpotContestMark | null

/** Build the marker for one contest snapshot; `null` when no contest runs. */
export function contestSpotMarker(fieldDay: FieldDayStatus | null | undefined): SpotMarker | null {
  if (fieldDay == null) return null
  // Narrowing the log to the spot's own call changes no answer only when the rule keys on the
  // call — every shipped rule does, and an older station's legacy triple does too. A rule that
  // did not would be judged against the whole log, exactly as the strip judges it.
  const byCall = fieldDay.dupeRule?.byCall ?? true
  const rows = new Map<string, FieldDayQso[]>()
  if (byCall) {
    for (const q of fieldDay.log ?? []) {
      const call = q.call.trim().toUpperCase()
      const list = rows.get(call)
      if (list) list.push(q)
      else rows.set(call, [q])
    }
  }
  const dupes = new Map<string, boolean>()
  return (s) => {
    const call = s.call.trim().toUpperCase()
    const cls = spotContestClass(s.mode)
    const key = `${call}\u0001${s.band}\u0001${cls}`
    let dupe = dupes.get(key)
    if (dupe === undefined) {
      const log = byCall ? { ...fieldDay, log: rows.get(call) ?? [] } : fieldDay
      dupe = contestDupe(log, call, s.band, cls) === 'own'
      dupes.set(key, dupe)
    }
    const mults = dupe ? [] : (s.contestMults ?? [])
    return dupe || mults.length > 0 ? { dupe, mults } : null
  }
}

/** [`contestSpotMarker`] memoised on the snapshot. */
export function useContestSpotMarker(fieldDay: FieldDayStatus | null | undefined): SpotMarker | null {
  return useMemo(() => contestSpotMarker(fieldDay), [fieldDay])
}

/** The words for a mark, for a tooltip and an accessible name: "dupe", "new mult: county COOK". */
export function spotMarkWords(mark: SpotContestMark | null | undefined): string[] {
  if (!mark) return []
  return [
    ...(mark.dupe ? [t('contestSpot.dupe')] : []),
    ...mark.mults.map((m) => t('contestSpot.mult', { rule: m.id, value: m.value })),
  ]
}
