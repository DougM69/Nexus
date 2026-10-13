// ⭐ A CONTEST SWITCHED BY A SAVE, as the station's snapshots show it. With Contest mode on, saving
// a different pick in Settings ▸ Contesting ends the running contest and starts the new one in the
// same save (`Engine::save_switches_contest`), so the snapshots go from one contest straight to the
// other. Turning the mode off and on passes through a snapshot with no contest, and a window that
// opens on a contest has seen none before it: neither is a switch, and neither is said.
//
// Read off what the engine did, never off what the form asked for: a pick the station cannot start
// yet ends the old contest and starts nothing, and the Contest screen's start blocker says what is
// missing. So the Contest screen can say it switched only when it did.
//
// Kept above the screen (App, the contest logger window): the pick is saved in Settings, where the
// Contest screen is not mounted, and it says so when the operator opens it.

import { useEffect, useState } from 'react'
import type { FieldDayStatus } from './types'

/** The contest the save left, and the one it started (rules-file ids, blank read as ARRL Field Day). */
export interface ContestSwitch {
  from: string
  to: string
}

/** What the tracker holds between snapshots: the contest the last one ran (`null` outside one) and
 *  the switch to say. */
export interface ContestSeen {
  last: string | null
  switched: ContestSwitch | null
}

/** The running contest's id, blank read as ARRL Field Day, as everywhere else; `null` outside one. */
export const runningContest = (fieldDay: FieldDayStatus | null | undefined): string | null =>
  fieldDay ? fieldDay.event || 'arrlfd' : null

/** One snapshot's step: a contest that follows another directly is a switch, a snapshot outside a
 *  contest forgets both, and the same contest keeps what was said. */
export function followContest(seen: ContestSeen, now: string | null): ContestSeen {
  if (now === null) return { last: null, switched: null }
  if (seen.last !== null && seen.last !== now) return { last: now, switched: { from: seen.last, to: now } }
  return { last: now, switched: seen.switched }
}

/** The switch to say for the session running now, or `null`. */
export function useContestSwitch(fieldDay: FieldDayStatus | null | undefined): ContestSwitch | null {
  const now = runningContest(fieldDay)
  const [seen, setSeen] = useState<ContestSeen>({ last: now, switched: null })
  useEffect(() => setSeen((s) => followContest(s, now)), [now])
  return seen.switched && seen.switched.to === now ? seen.switched : null
}
