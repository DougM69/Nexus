// The activator's side of POTA, as the screen reads it — no React, no IO, fully node-testable.
//
// What the station counts lives in the hot index (`tempo_core::logbook::hot::pota_key`): the
// contacts of one park on one UTC day, each `(call, band, mode, their park)` once. The warn-only
// dupe badge in the log form asks the SAME rule of the prior contacts it already holds for the
// typed call, so the badge and the counter cannot disagree about what a dupe is. Keep the two
// in step.

import type { Activation, LoggedQso } from '../types'
import { modeKey } from './callHistory'

/** Contacts POTA requires from one park in one UTC day for a valid activation. */
export const POTA_VALID_QSOS = 10

const DAY = 86_400

/** The parks of a reference list ("US-0001,US-0002"): split on the separators the export reads,
 *  trimmed and upper-cased, empties dropped. */
export function parkRefs(list: string | null | undefined): string[] {
  return (list ?? '')
    .split(/[,;]/)
    .map((p) => p.trim().toUpperCase())
    .filter((p) => p !== '')
}

/** POTA's uniqueness key for a contact (`pota_key` in the hot index): the call exactly as logged
 *  (W1ABC/P is not W1ABC), the band, the mode through the log's fold (USB and LSB are SSB), and
 *  the other station's park list. */
export function potaKey(call: string, band: string, mode: string, theirRef: string | null | undefined): string {
  return [call.trim().toUpperCase(), band.trim().toLowerCase(), modeKey(mode), parkRefs(theirRef).join(',')].join('\u0001')
}

/** The parks of the activation running now; empty when not activating. */
export function activationParks(act: Activation | null | undefined): string[] {
  if (!act?.reference) return []
  return act.parks?.length ? act.parks.map((p) => p.reference) : parkRefs(act.reference)
}

export interface PotaDupe {
  /** The park of the activation the earlier contact was made from. */
  park: string
  /** The earlier contact. */
  qso: LoggedQso
}

/** ⭐ The warn-only POTA dupe: an earlier contact with this call, made TODAY (UTC) from a park
 *  of the activation running now, with the same band, mode and other station's park — one POTA
 *  counts once. `null` when not activating or there is none. It never blocks: the hunter may
 *  still need the contact, and the upload check drops a dupe harmlessly.
 *
 *  `qsos` are the typed call's prior contacts (the history answer's — the call already matches,
 *  trimmed and upper-cased); `call` is checked again so a caller cannot pass another's. */
export function potaDupe(
  qsos: readonly LoggedQso[],
  act: Activation | null | undefined,
  entry: { call: string; band: string; mode: string; theirRef?: string | null; nowUnix: number },
): PotaDupe | null {
  const parks = activationParks(act)
  if (parks.length === 0 || !entry.band) return null
  const day = Math.floor(entry.nowUnix / DAY)
  const want = potaKey(entry.call, entry.band, entry.mode, entry.theirRef)
  let found: PotaDupe | null = null
  for (const q of qsos) {
    if (Math.floor(q.whenUnix / DAY) !== day) continue
    const park = parkRefs(q.ota?.myRef).find((p) => parks.includes(p))
    if (!park) continue
    if (potaKey(q.call, q.band ?? '', q.mode ?? '', q.ota?.theirRef) !== want) continue
    // The latest, so the badge names the contact the operator most likely remembers.
    if (!found || q.whenUnix > found.qso.whenUnix) found = { park, qso: q }
  }
  return found
}

/** One park's standing today: its distinct contacts, how many more make it valid, and whether it
 *  is valid. */
export function parkProgress(count: number): { count: number; toGo: number; valid: boolean } {
  return { count, toGo: Math.max(0, POTA_VALID_QSOS - count), valid: count >= POTA_VALID_QSOS }
}

/** A legacy US park reference ("K-1234"): POTA renamed US parks to "US-" in 2024, and whether
 *  the upload still takes the old prefix is not known, so the screen says so. */
export function isLegacyUsRef(ref: string): boolean {
  return /^K-\d{4,5}$/.test(ref.trim().toUpperCase())
}
